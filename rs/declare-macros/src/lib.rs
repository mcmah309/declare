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

/// Whether a `#[newtype]` annotation asks for struct generation or defers to a
/// foreign (already-existing) struct.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NewtypeKind {
    /// `#[newtype]` — generate the struct alongside the enum.
    Generate,
    /// `#[newtype(foreign)]` — the struct already exists elsewhere; only emit
    /// conversions/accessors/traits, not the struct definition itself.
    Foreign,
}

/// Inspect a single attribute.  Returns `Some(NewtypeKind)` when the attribute
/// is `#[newtype]` or `#[newtype(foreign)]`, `None` for anything else.
fn parse_newtype_attr(attr: &Attribute) -> Option<NewtypeKind> {
    if !attr.path().is_ident("newtype") {
        return None;
    }
    match &attr.meta {
        // Plain `#[newtype]`
        syn::Meta::Path(_) => Some(NewtypeKind::Generate),
        // `#[newtype(…)]`
        syn::Meta::List(ml) => {
            let token_str = ml.tokens.to_string();
            match token_str.trim() {
                "foreign" => Some(NewtypeKind::Foreign),
                other => panic!(
                    "unknown `#[newtype]` option `{other}`; \
                     expected `#[newtype]` or `#[newtype(foreign)]`"
                ),
            }
        }
        _ => panic!(
            "unexpected `#[newtype]` attribute form; \
             expected `#[newtype]` or `#[newtype(foreign)]`"
        ),
    }
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

/// Return a subset of `generics` containing only the params whose names appear
/// in `used`, together with any where-clause predicates that reference them.
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

/// Build the `<'a, T>` argument list for use-sites (no bounds).
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

/// `snake_case` → `PascalCase`  (e.g. `my_field` → `MyField`).
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
    /// Newtype variant — the inner struct is either generated (`kind ==
    /// Generate`) or expected to already exist (`kind == Foreign`).
    NewType {
        /// How the struct came to exist.
        newtype_kind: NewtypeKind,
        /// Variable name used in match patterns, e.g. `text` for `Text`.
        binding: Ident,
        /// Logical fields (used for accessor / trait generation).
        fields: Vec<FieldInfo>,
        /// Generics that the struct definition carries (filtered from the
        /// enum's generics based on which the struct's fields actually use).
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

// ============================================================================
// Type-level helpers
// ============================================================================

/// Strip `Option<T>` → `(true, T)`, otherwise `(false, original)`.
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

/// Strip `&'a T` / `&mut T` → `(true, T)`, otherwise `(false, original)`.
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
        // Look for `#[newtype]` or `#[newtype(foreign)]`.
        let newtype_attr = variant
            .attrs
            .iter()
            .enumerate()
            .find_map(|(i, a)| parse_newtype_attr(a).map(|k| (i, k)));

        if let Some((pos, newtype_kind)) = newtype_attr {
            if !config.newtype_variants {
                panic!(
                    "`#[newtype]` / `#[newtype(foreign)]` require `newtype_variants` to be enabled"
                );
            }
            variant.attrs.remove(pos);

            // Attributes remaining on the variant are forwarded to the
            // generated struct (only for generated structs; ignored for foreign structs).
            let extra_attrs = std::mem::take(&mut variant.attrs);

            let struct_ident = variant.ident.clone();

            let named: FieldsNamed = match &variant.fields {
                Fields::Named(f) => f.clone(),
                _ => panic!(
                    "`#[newtype]` / `#[newtype(foreign)]` are only supported \
                     on named-field variants"
                ),
            };

            let field_infos: Vec<FieldInfo> = named
                .named
                .iter()
                .map(|f| FieldInfo {
                    ident: f.ident.clone().unwrap(),
                    ty: f.ty.clone(),
                })
                .collect();

            // Work out which of the enum's generics this struct actually uses.
            let mut used = HashSet::new();
            for f in &named.named {
                used.extend(names_of(&f.ty));
            }
            let struct_generics = filter_generics(&enum_generics, &used);
            let use_args = use_site_generics(&struct_generics);

            if newtype_kind == NewtypeKind::Generate {
                let (struct_impl_g, _struct_ty_g, struct_where_g) =
                    struct_generics.split_for_impl();

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
            }

            // Rewrite the enum variant to `Variant(StructName<…>)`.
            variant.fields = Fields::Unnamed(parse_quote!((#struct_ident #use_args)));

            // Build a lifetime used as the borrow in the `TryFrom<&…>` impls.
            let internal_lifetime: syn::Lifetime = parse_quote!('declare_internal);
            let mut ref_generics = enum_generics.clone();
            ref_generics.params.insert(
                0,
                syn::GenericParam::Lifetime(syn::LifetimeParam::new(internal_lifetime.clone())),
            );
            let (ref_impl_g, _, _) = ref_generics.split_for_impl();

            // From<Struct> for Enum / TryFrom<Enum> for Struct
            conversions.push(quote! {
                impl #enum_impl_g ::core::convert::From<#struct_ident #use_args>
                    for #enum_ident #enum_ty_g #enum_where_g
                {
                    fn from(value: #struct_ident #use_args) -> Self {
                        #enum_ident::#struct_ident(value)
                    }
                }

                impl #enum_impl_g ::core::convert::TryFrom<#enum_ident #enum_ty_g>
                    for #struct_ident #use_args #enum_where_g
                {
                    type Error = #enum_ident #enum_ty_g;

                    fn try_from(
                        value: #enum_ident #enum_ty_g,
                    ) -> ::core::result::Result<Self, Self::Error> {
                        match value {
                            #enum_ident::#struct_ident(inner) => Ok(inner),
                            other => Err(other),
                        }
                    }
                }

                impl #ref_impl_g ::core::convert::TryFrom<
                    &#internal_lifetime #enum_ident #enum_ty_g
                > for &#internal_lifetime #struct_ident #use_args #enum_where_g
                {
                    type Error = &#internal_lifetime #enum_ident #enum_ty_g;

                    fn try_from(
                        value: &#internal_lifetime #enum_ident #enum_ty_g,
                    ) -> ::core::result::Result<Self, Self::Error> {
                        match value {
                            #enum_ident::#struct_ident(inner) => Ok(inner),
                            other => Err(other),
                        }
                    }
                }

                impl #ref_impl_g ::core::convert::TryFrom<
                    &#internal_lifetime mut #enum_ident #enum_ty_g
                > for &#internal_lifetime mut #struct_ident #use_args #enum_where_g
                {
                    type Error = &#internal_lifetime #enum_ident #enum_ty_g;

                    fn try_from(
                        value: &#internal_lifetime mut #enum_ident #enum_ty_g,
                    ) -> ::core::result::Result<Self, Self::Error> {
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
                    newtype_kind,
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
                    panic!(
                        "tuple variants without `#[newtype]` / `#[newtype(foreign)]` are not supported"
                    )
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

    if config.common_accessors {
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

        // All present occurrences must agree on a base type.
        let base_ty = unified_base_type(&presences);
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
                match self { #(#ref_arms)* }
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
                    match self { #(#mut_arms)* }
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
                    match self { #(#into_arms)* }
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

//************************************************************************//

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

        let base_ty = unified_base_type(&presences);
        let Some(base_ty) = base_ty else { continue };

        // No guaranteed non-optional accessor anywhere → skip.
        if presences.iter().flatten().all(|p| p.is_option) {
            continue;
        }

        let any_reference = presences.iter().flatten().any(|p| p.is_reference);
        let any_absent = presences.iter().any(|p| p.is_none());
        let any_option = presences.iter().flatten().any(|p| p.is_option);
        let enum_fully_present = !any_absent && !any_option;

        // Generics needed for the field's base type — used in trait decls and
        // all impl blocks that reference this trait.
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

        // enum impl (only when every variant carries the field non-optionally)
        if enum_fully_present {
            let ref_arms = build_arms(
                enum_ident,
                variants,
                &field_ident,
                &presences,
                false,
                Mode::Ref,
            );
            out.extend(quote! {
                impl #enum_impl_g #ref_trait_ident #use_field_args
                    for #enum_ident #enum_ty_g #enum_where_g
                {
                    fn #ref_name(&self) -> &#base_ty {
                        match self { #(#ref_arms)* }
                    }
                }
            });

            if !any_reference {
                let mut_arms = build_arms(
                    enum_ident,
                    variants,
                    &field_ident,
                    &presences,
                    false,
                    Mode::Mut,
                );
                let into_arms = build_arms(
                    enum_ident,
                    variants,
                    &field_ident,
                    &presences,
                    false,
                    Mode::Into,
                );
                out.extend(quote! {
                    impl #enum_impl_g #mut_trait_ident #use_field_args
                        for #enum_ident #enum_ty_g #enum_where_g
                    {
                        fn #mut_name(&mut self) -> &mut #base_ty {
                            match self { #(#mut_arms)* }
                        }
                    }
                    impl #enum_impl_g #into_trait_ident #use_field_args
                        for #enum_ident #enum_ty_g #enum_where_g
                    {
                        fn #into_name(self) -> #base_ty {
                            match self { #(#into_arms)* }
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

            let ref_body = if p.is_reference {
                // Field is already `&'a T`; returning it satisfies `&T`.
                quote!(self.#field_ident)
            } else {
                quote!(&self.#field_ident)
            };
            out.extend(quote! {
                impl #struct_impl_g #ref_trait_ident #use_field_args
                    for #struct_ident #struct_ty_g #struct_where_g
                {
                    fn #ref_name(&self) -> &#base_ty {
                        #ref_body
                    }
                }
            });

            if !any_reference {
                out.extend(quote! {
                    impl #struct_impl_g #mut_trait_ident #use_field_args
                        for #struct_ident #struct_ty_g #struct_where_g
                    {
                        fn #mut_name(&mut self) -> &mut #base_ty {
                            &mut self.#field_ident
                        }
                    }
                    impl #struct_impl_g #into_trait_ident #use_field_args
                        for #struct_ident #struct_ty_g #struct_where_g
                    {
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

//************************************************************************//

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
                // Collect every absent variant's wildcard pattern into one arm.
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
            // Match ergonomics bind `field_ident` as `&T` / `&mut T` / `T`.
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

/// Return the single base type that all present `Presence` entries agree on,
/// or `None` if they disagree (meaning no unified accessor is possible).
fn unified_base_type(presences: &[Option<Presence>]) -> Option<Type> {
    let mut found: Option<Type> = None;
    for p in presences.iter().flatten() {
        match &found {
            None => found = Some(p.base.clone()),
            Some(t) => {
                if !type_eq(t, &p.base) {
                    return None;
                }
            }
        }
    }
    found
}
