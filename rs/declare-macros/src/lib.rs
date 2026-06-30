use std::collections::HashSet;

use proc_macro::TokenStream;
use proc_macro2::{TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, quote};
use syn::{
    Attribute, Fields, FieldsNamed, GenericParam, Generics, Ident, ItemEnum, PathArguments, Token,
    Type, Visibility, WhereClause,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
};

#[derive(Default, Clone)]
struct DeclareConfig {
    newtype_variants: bool,
    common_accessors: bool,
    field_traits: bool,
}

impl Parse for DeclareConfig {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let idents = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
        let mut config = DeclareConfig::default();
        for id in idents {
            match id.to_string().as_str() {
                "newtype_variants" => config.newtype_variants = true,
                "common_accessors" => config.common_accessors = true,
                "field_traits" => config.field_traits = true,
                other => {
                    return Err(syn::Error::new(
                        id.span(),
                        format!("unknown `declare` option `{other}`"),
                    ));
                }
            }
        }
        Ok(config)
    }
}

fn collect_declare_macros(initial: &mut DeclareConfig, attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| {
        let path = attr.path();
        // Ensure there are no generic arguments in the entire path
        if path
            .segments
            .iter()
            .any(|s| !matches!(s.arguments, PathArguments::None))
        {
            return true;
        }
        let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();

        match segments
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["augment"] | ["declare", "augment"] => {
                let config: DeclareConfig = attr.parse_args().unwrap_or_default();
                initial.newtype_variants |= config.newtype_variants;
                initial.common_accessors |= config.common_accessors;
                initial.field_traits |= config.field_traits;
                false
            }
            ["newtype_variants"] | ["declare", "newtype_variants"] => {
                initial.newtype_variants = true;
                false
            }
            ["common_accessors"] | ["declare", "common_accessors"] => {
                initial.common_accessors = true;
                false
            }
            ["field_traits"] | ["declare", "field_traits"] => {
                initial.field_traits = true;
                false
            }
            // Any other attribute (e.g., #[derive(...)], #[inline])
            _ => true,
        }
    });
}

//************************************************************************//

fn collect_names(ts: TokenStream2, names: &mut HashSet<String>) {
    let mut iter = ts.into_iter().peekable();
    while let Some(tt) = iter.next() {
        match tt {
            TokenTree::Group(g) => collect_names(g.stream(), names),
            TokenTree::Ident(id) => {
                names.insert(id.to_string());
            }
            TokenTree::Punct(p) if p.as_char() == '\'' => {
                if let Some(TokenTree::Ident(id)) = iter.peek() {
                    names.insert(format!("'{id}"));
                    iter.next();
                }
            }
            _ => {}
        }
    }
}

fn names_of<T: ToTokens>(t: &T) -> HashSet<String> {
    let mut s = HashSet::new();
    collect_names(quote!(#t), &mut s);
    s
}

//************************************************************************//

fn param_name(p: &GenericParam) -> String {
    match p {
        GenericParam::Lifetime(lp) => format!("'{}", lp.lifetime.ident),
        GenericParam::Type(tp) => tp.ident.to_string(),
        GenericParam::Const(cp) => cp.ident.to_string(),
    }
}

fn filter_generics(generics: &Generics, used: &HashSet<String>) -> Generics {
    let mut new_generics = Generics::default();

    for param in &generics.params {
        if used.contains(&param_name(param)) {
            new_generics.params.push(param.clone());
        }
    }

    if let Some(wc) = &generics.where_clause {
        let preds: Punctuated<_, Token![,]> = wc
            .predicates
            .iter()
            .filter(|pred| {
                let pred_names = names_of(pred);
                pred_names.intersection(used).next().is_some()
            })
            .cloned()
            .collect();

        if !preds.is_empty() {
            new_generics.where_clause = Some(WhereClause {
                where_token: wc.where_token,
                predicates: preds,
            });
        }
    }

    new_generics
}

/// Build the `<'a, T>` style argument list used at a *use site* (no bounds).
fn use_site_generics(generics: &Generics) -> TokenStream2 {
    if generics.params.is_empty() {
        return quote!();
    }
    let args = generics.params.iter().map(|p| match p {
        GenericParam::Lifetime(lp) => {
            let lt = &lp.lifetime;
            quote!(#lt)
        }
        GenericParam::Type(tp) => {
            let i = &tp.ident;
            quote!(#i)
        }
        GenericParam::Const(cp) => {
            let i = &cp.ident;
            quote!(#i)
        }
    });
    quote!(<#(#args),*>)
}

/// Convert a `snake_case` field name into `PascalCase` for trait naming
/// (e.g. `into_a` field-derived trait piece -> `A`, used as `ARef`, `AMut`, `IntoA`).
fn pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

//************************************************************************//

#[derive(Clone)]
struct FieldInfo {
    ident: Ident,
    ty: Type,
}

#[derive(Clone)]
enum VariantKind {
    /// Generated from `#[newtype]`: `Variant(StructName<...>)`.
    NewType {
        binding: Ident,
        fields: Vec<FieldInfo>,
        /// The generics actually declared on the generated struct (a filtered
        /// subset of the enum's generics, based on what the struct's fields use).
        generics: Generics,
    },
    /// A plain `Variant { a: T, b: U }` left untouched.
    InlineNamed { fields: Vec<FieldInfo> },
    /// `Variant` with no data.
    Unit,
}

#[derive(Clone)]
struct VariantInfo {
    ident: Ident,
    kind: VariantKind,
}

impl VariantInfo {
    fn field(&self, name: &str) -> Option<&FieldInfo> {
        match &self.kind {
            VariantKind::NewType { fields, .. } | VariantKind::InlineNamed { fields } => {
                fields.iter().find(|f| f.ident == name)
            }
            VariantKind::Unit => None,
        }
    }
}

/// Strip `Option<T>` -> `(true, T)`, otherwise `(false, original)`.
fn unwrap_option(ty: &Type) -> (bool, Type) {
    if let Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
        && seg.ident == "Option"
        && let syn::PathArguments::AngleBracketed(ab) = &seg.arguments
        && let Some(syn::GenericArgument::Type(inner)) = ab.args.first()
    {
        return (true, inner.clone());
    }
    (false, ty.clone())
}

/// Strip `&'a T` / `&mut T` -> `(true, T)`, otherwise `(false, original)`.
fn unwrap_reference(ty: &Type) -> (bool, Type) {
    if let Type::Reference(r) = ty {
        return (true, (*r.elem).clone());
    }
    (false, ty.clone())
}

struct Presence {
    is_option: bool,
    is_reference: bool,
    base: Type,
}

fn type_eq(a: &Type, b: &Type) -> bool {
    quote!(#a).to_string() == quote!(#b).to_string()
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ref,
    Mut,
    Into,
}

//************************************************************************//

#[proc_macro_attribute]
pub fn newtype_variants(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemEnum);

    let mut config = DeclareConfig {
        newtype_variants: true,
        common_accessors: false,
        field_traits: false,
    };
    collect_declare_macros(&mut config, &mut input.attrs);
    expand_fully(input, &config)
}

#[proc_macro_attribute]
pub fn common_accessors(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemEnum);

    let mut config = DeclareConfig {
        newtype_variants: false,
        common_accessors: true,
        field_traits: false,
    };
    collect_declare_macros(&mut config, &mut input.attrs);
    expand_fully(input, &config)
}

#[proc_macro_attribute]
pub fn field_traits(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemEnum);

    let mut config = DeclareConfig {
        newtype_variants: false,
        common_accessors: false,
        field_traits: true,
    };
    collect_declare_macros(&mut config, &mut input.attrs);
    expand_fully(input, &config)
}

#[doc(hidden)]
#[proc_macro_attribute]
pub fn augment(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = parse_macro_input!(attr as DeclareConfig);
    let input = parse_macro_input!(item as ItemEnum);
    expand_fully(input, &config)
}

fn expand_fully(mut input: ItemEnum, config: &DeclareConfig) -> TokenStream {
    let enum_vis = input.vis.clone();
    let enum_ident = input.ident.clone();
    let enum_generics = input.generics.clone();
    let (enum_impl_g, enum_ty_g, enum_where_g) = enum_generics.split_for_impl();

    let mut generated_structs: Vec<TokenStream2> = Vec::new();
    let mut conversions: Vec<TokenStream2> = Vec::new();
    let mut variant_infos: Vec<VariantInfo> = Vec::new();

    for variant in input.variants.iter_mut() {
        let struct_attr_pos = variant
            .attrs
            .iter()
            .position(|a| a.path().is_ident("newtype"));

        if let Some(pos) = struct_attr_pos {
            if !config.newtype_variants {
                panic!("`#[newtype]` requires `newtype_variants` to be enabled");
            }
            variant.attrs.remove(pos);

            // Everything left over (e.g. #[derive(Debug, Clone)]) is
            // forwarded onto the generated struct.
            let extra_attrs = std::mem::take(&mut variant.attrs);

            let struct_ident = variant.ident.clone();

            let named: FieldsNamed = match &variant.fields {
                Fields::Named(f) => f.clone(),
                _ => panic!("#[newtype] is only supported on named-field variants"),
            };

            let field_infos: Vec<FieldInfo> = named
                .named
                .iter()
                .map(|f| FieldInfo {
                    ident: f.ident.clone().unwrap(),
                    ty: f.ty.clone(),
                })
                .collect();

            // Work out which of the enum's generics/lifetimes this
            // particular struct actually needs.
            let mut used = HashSet::new();
            for f in &named.named {
                used.extend(names_of(&f.ty));
            }
            let struct_generics = filter_generics(&enum_generics, &used);
            let (struct_impl_g, _struct_ty_g, struct_where_g) = struct_generics.split_for_impl();
            let mut fields = syn::punctuated::Punctuated::<syn::Field, syn::Token![,]>::new();
            for f in named.named.iter() {
                let mut f = f.clone();
                f.vis = enum_vis.clone();
                fields.push(f);
            }

            generated_structs.push(quote! {
                #(#extra_attrs)*
                #enum_vis struct #struct_ident #struct_impl_g #struct_where_g {
                    #fields
                }
            });

            // Rewrite the enum variant as a newtype: `W(W<T>)`.
            let use_args = use_site_generics(&struct_generics);
            variant.fields = Fields::Unnamed(parse_quote!((#struct_ident #use_args)));

            let internal_lifetime: syn::Lifetime = parse_quote!('declare_internal);
            let mut ref_generics = enum_generics.clone();
            ref_generics.params.insert(
                0,
                syn::GenericParam::Lifetime(syn::LifetimeParam::new(internal_lifetime.clone())),
            );
            let (ref_impl_g, _, _) = ref_generics.split_for_impl();

            // From<Struct> for Enum / TryFrom<Enum> for Struct
            conversions.push(quote! {
                impl #enum_impl_g ::core::convert::From<#struct_ident #use_args> for #enum_ident #enum_ty_g #enum_where_g {
                    fn from(value: #struct_ident #use_args) -> Self {
                        #enum_ident::#struct_ident(value)
                    }
                }

                impl #enum_impl_g ::core::convert::TryFrom<#enum_ident #enum_ty_g> for #struct_ident #use_args #enum_where_g {
                    type Error = #enum_ident #enum_ty_g;

                    fn try_from(value: #enum_ident #enum_ty_g) -> ::core::result::Result<Self, Self::Error> {
                        match value {
                            #enum_ident::#struct_ident(inner) => Ok(inner),
                            other => Err(other),
                        }
                    }
                }

                impl #ref_impl_g ::core::convert::TryFrom<&#internal_lifetime #enum_ident #enum_ty_g> for &#internal_lifetime #struct_ident #use_args #enum_where_g {
                    type Error = &#internal_lifetime #enum_ident #enum_ty_g;

                    fn try_from(value: &#internal_lifetime #enum_ident #enum_ty_g) -> ::core::result::Result<Self, Self::Error> {
                        match value {
                            #enum_ident::#struct_ident(inner) => Ok(inner),
                            other => Err(other),
                        }
                    }
                }

                impl #ref_impl_g ::core::convert::TryFrom<&#internal_lifetime mut #enum_ident #enum_ty_g> for &#internal_lifetime mut #struct_ident #use_args #enum_where_g {
                    type Error = &#internal_lifetime #enum_ident #enum_ty_g;

                    fn try_from(value: &#internal_lifetime mut #enum_ident #enum_ty_g) -> ::core::result::Result<Self, Self::Error> {
                        match value {
                            #enum_ident::#struct_ident(inner) => Ok(inner),
                            other => Err(other),
                        }
                    }
                }
            });

            let binding = Ident::new(
                &struct_ident.to_string().to_lowercase(),
                struct_ident.span(),
            );
            variant_infos.push(VariantInfo {
                ident: struct_ident,
                kind: VariantKind::NewType {
                    binding,
                    fields: field_infos,
                    generics: struct_generics,
                },
            });
        } else {
            match &variant.fields {
                Fields::Named(f) => {
                    let fields = f
                        .named
                        .iter()
                        .map(|fld| FieldInfo {
                            ident: fld.ident.clone().unwrap(),
                            ty: fld.ty.clone(),
                        })
                        .collect();
                    variant_infos.push(VariantInfo {
                        ident: variant.ident.clone(),
                        kind: VariantKind::InlineNamed { fields },
                    });
                }
                Fields::Unit => variant_infos.push(VariantInfo {
                    ident: variant.ident.clone(),
                    kind: VariantKind::Unit,
                }),
                Fields::Unnamed(_) => {
                    panic!("tuple variants without `#[newtype]` are not supported")
                }
            }
        }
    }

    let mut output = quote! { #input };
    for s in generated_structs {
        output.extend(s);
    }
    for c in conversions {
        output.extend(c);
    }

    // `field_traits` impls delegate to the enum's `_ref`/`_mut`/`into_` accessor
    // methods, so generating those accessors is implied.
    let needs_common_accessors = config.common_accessors || config.field_traits;

    if needs_common_accessors {
        output.extend(generate_accessors(
            &enum_vis,
            &enum_ident,
            &enum_generics,
            &variant_infos,
        ));
    }

    if config.field_traits {
        output.extend(generate_field_traits(
            &enum_vis,
            &enum_ident,
            &enum_generics,
            &variant_infos,
        ));
    }

    output.into()
}

//************************************************************************//

fn generate_accessors(
    enum_vis: &Visibility,
    enum_ident: &Ident,
    enum_generics: &Generics,
    variants: &[VariantInfo],
) -> TokenStream2 {
    // Collect field names in first-seen order across all variants.
    let mut field_names: Vec<String> = Vec::new();
    for v in variants {
        let fields = match &v.kind {
            VariantKind::NewType { fields, .. } | VariantKind::InlineNamed { fields } => fields,
            VariantKind::Unit => continue,
        };
        for f in fields {
            let name = f.ident.to_string();
            if !field_names.contains(&name) {
                field_names.push(name);
            }
        }
    }

    let mut methods = Vec::new();

    for name in field_names {
        let field_ident = Ident::new(&name, proc_macro2::Span::call_site());

        let presences: Vec<Option<Presence>> = variants
            .iter()
            .map(|v| {
                v.field(&name).map(|f| {
                    let (is_opt, after_opt) = unwrap_option(&f.ty);
                    let (is_ref, base) = unwrap_reference(&after_opt);
                    Presence {
                        is_option: is_opt,
                        is_reference: is_ref,
                        base,
                    }
                })
            })
            .collect();

        // Make sure every present variant agrees on the base type;
        // otherwise we can't sensibly unify and skip this field.
        let base_ty: Option<Type> = {
            let mut found: Option<Type> = None;
            let mut ok = true;
            for p in presences.iter().flatten() {
                match &found {
                    None => found = Some(p.base.clone()),
                    Some(t) => {
                        if !type_eq(t, &p.base) {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            if ok { found } else { None }
        };

        let Some(base_ty) = base_ty else { continue };

        let any_absent = presences.iter().any(|p| p.is_none());
        let any_option = presences.iter().flatten().any(|p| p.is_option);
        let any_reference = presences.iter().flatten().any(|p| p.is_reference);
        let result_optional = any_absent || any_option;

        let ref_ret = if result_optional {
            quote!(Option<&#base_ty>)
        } else {
            quote!(&#base_ty)
        };
        let mut_ret = if result_optional {
            quote!(Option<&mut #base_ty>)
        } else {
            quote!(&mut #base_ty)
        };
        let into_ret = if result_optional {
            quote!(Option<#base_ty>)
        } else {
            quote!(#base_ty)
        };

        let ref_name = Ident::new(&format!("{name}_ref"), field_ident.span());
        let mut_name = Ident::new(&format!("{name}_mut"), field_ident.span());
        let into_name = Ident::new(&format!("into_{name}"), field_ident.span());

        let ref_arms = build_arms(
            enum_ident,
            variants,
            &field_ident,
            &presences,
            result_optional,
            Mode::Ref,
        );
        methods.push(quote! {
            #enum_vis fn #ref_name(&self) -> #ref_ret {
                match self {
                    #(#ref_arms)*
                }
            }
        });

        if !any_reference {
            let mut_arms = build_arms(
                enum_ident,
                variants,
                &field_ident,
                &presences,
                result_optional,
                Mode::Mut,
            );
            methods.push(quote! {
                #enum_vis fn #mut_name(&mut self) -> #mut_ret {
                    match self {
                        #(#mut_arms)*
                    }
                }
            });

            let into_arms = build_arms(
                enum_ident,
                variants,
                &field_ident,
                &presences,
                result_optional,
                Mode::Into,
            );
            methods.push(quote! {
                #enum_vis fn #into_name(self) -> #into_ret {
                    match self {
                        #(#into_arms)*
                    }
                }
            });
        }
    }

    let (impl_g, ty_g, where_g) = enum_generics.split_for_impl();
    quote! {
        impl #impl_g #enum_ident #ty_g #where_g {
            #(#methods)*
        }
    }
}

fn build_arms(
    enum_ident: &Ident,
    variants: &[VariantInfo],
    field_ident: &Ident,
    presences: &[Option<Presence>],
    result_optional: bool,
    mode: Mode,
) -> Vec<TokenStream2> {
    let mut arms = Vec::new();
    let mut absent_emitted = false;

    for (i, v) in variants.iter().enumerate() {
        match &presences[i] {
            None => {
                if absent_emitted {
                    continue;
                }
                absent_emitted = true;
                // Collect every absent variant's wildcard pattern.
                let pats: Vec<TokenStream2> = variants
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| presences[*j].is_none())
                    .map(|(_, v2)| absent_pattern(enum_ident, v2))
                    .collect();
                arms.push(quote! { #(#pats)|* => None, });
            }
            Some(p) => {
                let pat = present_pattern(enum_ident, v, field_ident);
                let body = present_body(v, field_ident, p, result_optional, mode);
                arms.push(quote! { #pat => #body, });
            }
        }
    }

    arms
}

fn absent_pattern(enum_ident: &Ident, v: &VariantInfo) -> TokenStream2 {
    let vi = &v.ident;
    match &v.kind {
        VariantKind::NewType { .. } => quote!(#enum_ident::#vi(_)),
        VariantKind::InlineNamed { .. } => quote!(#enum_ident::#vi { .. }),
        VariantKind::Unit => quote!(#enum_ident::#vi),
    }
}

fn present_pattern(enum_ident: &Ident, v: &VariantInfo, field_ident: &Ident) -> TokenStream2 {
    let vi = &v.ident;
    match &v.kind {
        VariantKind::NewType { binding, .. } => quote!(#enum_ident::#vi(#binding)),
        VariantKind::InlineNamed { .. } => quote!(#enum_ident::#vi { #field_ident, .. }),
        VariantKind::Unit => unreachable!("unit variants never have a present field"),
    }
}

fn present_body(
    v: &VariantInfo,
    field_ident: &Ident,
    p: &Presence,
    result_optional: bool,
    mode: Mode,
) -> TokenStream2 {
    match &v.kind {
        VariantKind::NewType { binding, .. } => {
            if p.is_reference {
                // Reference fields only ever get a `_ref` accessor.
                if result_optional {
                    quote!(Some(#binding.#field_ident))
                } else {
                    quote!(#binding.#field_ident)
                }
            } else if p.is_option {
                match mode {
                    Mode::Ref => quote!(#binding.#field_ident.as_ref()),
                    Mode::Mut => quote!(#binding.#field_ident.as_mut()),
                    Mode::Into => quote!(#binding.#field_ident),
                }
            } else {
                match mode {
                    Mode::Ref if result_optional => quote!(Some(&#binding.#field_ident)),
                    Mode::Ref => quote!(&#binding.#field_ident),
                    Mode::Mut if result_optional => quote!(Some(&mut #binding.#field_ident)),
                    Mode::Mut => quote!(&mut #binding.#field_ident),
                    Mode::Into if result_optional => quote!(Some(#binding.#field_ident)),
                    Mode::Into => quote!(#binding.#field_ident),
                }
            }
        }
        VariantKind::InlineNamed { .. } => {
            // Thanks to match ergonomics, `field_ident` is already bound
            // as `&T` / `&mut T` / `T` depending on `mode`.
            if p.is_reference {
                if result_optional {
                    quote!(Some(*#field_ident))
                } else {
                    quote!(*#field_ident)
                }
            } else if p.is_option {
                match mode {
                    Mode::Ref => quote!(#field_ident.as_ref()),
                    Mode::Mut => quote!(#field_ident.as_mut()),
                    Mode::Into => quote!(#field_ident),
                }
            } else if result_optional {
                quote!(Some(#field_ident))
            } else {
                quote!(#field_ident)
            }
        }
        VariantKind::Unit => unreachable!(),
    }
}

//************************************************************************//

/// `field_traits`: per-field traits (`<Field>Ref` / `<Field>Mut` / `Into<Field>`)
/// implemented by every `#[newtype]` struct that has the field non-optionally,
/// and by the enum itself when *every* variant has the field non-optionally.
fn generate_field_traits(
    enum_vis: &Visibility,
    enum_ident: &Ident,
    enum_generics: &Generics,
    variants: &[VariantInfo],
) -> TokenStream2 {
    let (enum_impl_g, enum_ty_g, enum_where_g) = enum_generics.split_for_impl();

    let mut field_names: Vec<String> = Vec::new();
    for v in variants {
        let fields = match &v.kind {
            VariantKind::NewType { fields, .. } | VariantKind::InlineNamed { fields } => fields,
            VariantKind::Unit => continue,
        };
        for f in fields {
            let name = f.ident.to_string();
            if !field_names.contains(&name) {
                field_names.push(name);
            }
        }
    }

    let mut out = TokenStream2::new();

    for name in field_names {
        let field_ident = Ident::new(&name, proc_macro2::Span::call_site());

        let presences: Vec<Option<Presence>> = variants
            .iter()
            .map(|v| {
                v.field(&name).map(|f| {
                    let (is_opt, after_opt) = unwrap_option(&f.ty);
                    let (is_ref, base) = unwrap_reference(&after_opt);
                    Presence {
                        is_option: is_opt,
                        is_reference: is_ref,
                        base,
                    }
                })
            })
            .collect();

        // Every present occurrence of the field must agree on a base type,
        // or there's no single trait signature to unify around.
        let base_ty: Option<Type> = {
            let mut found: Option<Type> = None;
            let mut ok = true;
            for p in presences.iter().flatten() {
                match &found {
                    None => found = Some(p.base.clone()),
                    Some(t) => {
                        if !type_eq(t, &p.base) {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            if ok { found } else { None }
        };
        let Some(base_ty) = base_ty else { continue };

        // If every present occurrence is `Option<...>`, there's no
        // guaranteed accessor anywhere to back a trait impl. Skip it.
        if presences.iter().flatten().all(|p| p.is_option) {
            continue;
        }

        let any_reference = presences.iter().flatten().any(|p| p.is_reference);
        let any_absent = presences.iter().any(|p| p.is_none());
        let any_option = presences.iter().flatten().any(|p| p.is_option);
        let enum_fully_present = !any_absent && !any_option;

        // Generics needed just for this field's base type (a subset of the
        // enum's generics), used both for the trait declaration and as the
        // trait's use-site arguments in every impl.
        let used = names_of(&base_ty);
        let field_generics = filter_generics(enum_generics, &used);
        let (field_impl_g, _field_ty_g, field_where_g) = field_generics.split_for_impl();
        let use_field_args = use_site_generics(&field_generics);

        let pascal = pascal_case(&name);
        let ref_trait_ident = Ident::new(&format!("{pascal}Ref"), field_ident.span());
        let mut_trait_ident = Ident::new(&format!("{pascal}Mut"), field_ident.span());
        let into_trait_ident = Ident::new(&format!("Into{pascal}"), field_ident.span());

        let ref_name = Ident::new(&format!("{name}_ref"), field_ident.span());
        let mut_name = Ident::new(&format!("{name}_mut"), field_ident.span());
        let into_name = Ident::new(&format!("into_{name}"), field_ident.span());

        // --- trait declarations ---
        out.extend(quote! {
            #enum_vis trait #ref_trait_ident #field_impl_g #field_where_g {
                fn #ref_name(&self) -> &#base_ty;
            }
        });
        if !any_reference {
            out.extend(quote! {
                #enum_vis trait #mut_trait_ident #field_impl_g #field_where_g {
                    fn #mut_name(&mut self) -> &mut #base_ty;
                }
                #enum_vis trait #into_trait_ident #field_impl_g #field_where_g {
                    fn #into_name(self) -> #base_ty;
                }
            });
        }

        // enum impls, delegating to the `common_accessors`-generated methods // todo remove dependence on common_accessors
        if enum_fully_present {
            out.extend(quote! {
                impl #enum_impl_g #ref_trait_ident #use_field_args for #enum_ident #enum_ty_g #enum_where_g {
                    fn #ref_name(&self) -> &#base_ty {
                        self.#ref_name()
                    }
                }
            });
            if !any_reference {
                out.extend(quote! {
                    impl #enum_impl_g #mut_trait_ident #use_field_args for #enum_ident #enum_ty_g #enum_where_g {
                        fn #mut_name(&mut self) -> &mut #base_ty {
                            self.#mut_name()
                        }
                    }
                    impl #enum_impl_g #into_trait_ident #use_field_args for #enum_ident #enum_ty_g #enum_where_g {
                        fn #into_name(self) -> #base_ty {
                            self.#into_name()
                        }
                    }
                });
            }
        }

        // per-newtype-struct impls, accessing the field directly
        for (i, v) in variants.iter().enumerate() {
            let VariantKind::NewType {
                generics: struct_generics,
                ..
            } = &v.kind
            else {
                continue;
            };
            let Some(p) = &presences[i] else { continue };
            if p.is_option {
                continue;
            }

            let struct_ident = &v.ident;
            let (struct_impl_g, struct_ty_g, struct_where_g) = struct_generics.split_for_impl();

            // `is_reference` fields are stored as `&'a T` already, so
            // returning the field itself satisfies `&T` via subtyping.
            let ref_body = if p.is_reference {
                quote!(self.#field_ident)
            } else {
                quote!(&self.#field_ident)
            };
            out.extend(quote! {
                impl #struct_impl_g #ref_trait_ident #use_field_args for #struct_ident #struct_ty_g #struct_where_g {
                    fn #ref_name(&self) -> &#base_ty {
                        #ref_body
                    }
                }
            });

            if !any_reference {
                out.extend(quote! {
                    impl #struct_impl_g #mut_trait_ident #use_field_args for #struct_ident #struct_ty_g #struct_where_g {
                        fn #mut_name(&mut self) -> &mut #base_ty {
                            &mut self.#field_ident
                        }
                    }
                    impl #struct_impl_g #into_trait_ident #use_field_args for #struct_ident #struct_ty_g #struct_where_g {
                        fn #into_name(self) -> #base_ty {
                            self.#field_ident
                        }
                    }
                });
            }
        }
    }

    out
}