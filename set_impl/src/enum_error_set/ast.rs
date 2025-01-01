use proc_macro2::TokenStream;
use syn::{
    braced, parenthesized,
    parse::{Parse, ParseBuffer, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
    token::{self},
    Attribute, Ident, Result, TypeParam,
};

const DISPLAY_ATTRIBUTE_NAME: &str = "display";
const DISABLE_ATTRIBUTE_NAME: &str = "disable";

#[derive(Clone)]
pub(crate) struct AstErrorSet {
    pub(crate) set_items: Vec<AstErrorDeclaration>,
}

impl Parse for AstErrorSet {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut set_items = Vec::new();

        while !input.is_empty() {
            let fork = input.fork();
            let set_item = match input.parse::<AstErrorDeclaration>() {
                Ok(value) => value,
                Err(error) => {
                    if input.is_empty() {
                        return Err(syn::Error::new(last_token_span(fork), error.to_string()));
                    } else {
                        return Err(error);
                    }
                }
            };
            set_items.push(set_item);
            if input.peek(token::Semi) {
                input.parse::<token::Semi>().unwrap();
            } else {
                if input.is_empty() {
                    return Err(syn::Error::new(
                        last_token_span(fork),
                        "Expected a `;` after an error definition.",
                    ));
                } else {
                    return Err(syn::Error::new(
                        input.span(),
                        "Expected a `;` after an error definition.",
                    ));
                }
            }
        }
        Ok(AstErrorSet { set_items })
    }
}

#[derive(Clone)]
pub(crate) struct AstErrorDeclaration {
    pub(crate) attributes: Vec<Attribute>,
    pub(crate) error_name: Ident,
    pub(crate) generics: Vec<TypeParam>,
    pub(crate) disabled: Disabled,
    pub(crate) parts: Vec<AstInlineOrRefError>,
}

impl Parse for AstErrorDeclaration {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut attributes = input.call(Attribute::parse_outer)?;
        let disabled = extract_disabled(&mut attributes)?;
        if input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                    "Expected an error definition to be next after attributes. You may have a dangling doc comment.",
            ));
        }
        let save_position = input.fork();
        let error_name: Ident = input.parse()?;
        if !input.peek(syn::Token![=]) && !input.peek(syn::Token![<]) {
            return Err(syn::Error::new(
                save_position.span(),
                "Expected `=` or generic `<..>` to be next next.",
            ));
        }
        let generics = generics(&input)?;
        let last_position_save = input.fork();
        if !input.peek(syn::Token![=]) {
            return Err(syn::Error::new(
                last_position_save.span(),
                "Expected `=` to be next.",
            ));
        }
        input.parse::<syn::Token![=]>().unwrap();
        let mut parts = Vec::new();
        while !input.is_empty() {
            let part = input.parse::<AstInlineOrRefError>()?;
            parts.push(part);
            if input.peek(token::OrOr) {
                input.parse::<token::OrOr>().unwrap();
                continue;
            } else if input.peek(token::Semi) {
                break;
            } else {
                return Err(syn::Error::new(
                    input.span(),
                    "Expected `||` or `;` to be next.",
                ));
            }
        }
        if parts.is_empty() {
            return Err(syn::Error::new(
                last_position_save.span(),
                "Missing error definitions",
            ));
        }
        return Ok(AstErrorDeclaration {
            attributes,
            error_name,
            generics,
            disabled,
            parts,
        });
    }
}

#[derive(Clone)]
pub(crate) enum AstInlineOrRefError {
    Inline(AstInlineError),
    Ref(RefError),
}

impl Parse for AstInlineOrRefError {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(token::Brace) {
            return match input.parse::<AstInlineError>() {
                Ok(inline_error) => Ok(AstInlineOrRefError::Inline(inline_error)),
                Err(err) => Err(err),
            };
        }
        if input.peek(Ident) {
            return match input.parse::<RefError>() {
                Ok(ref_error) => Ok(AstInlineOrRefError::Ref(ref_error)),
                Err(err) => Err(err),
            };
        }
        return Err(syn::parse::Error::new(
            input.span(),
            "Expected the next token to be the start of an inline error variant ('{...}') or a reference to another error enum.",
        ));
    }
}

#[derive(Clone)]
pub(crate) struct AstInlineError {
    pub error_variants: Punctuated<AstErrorVariant, token::Comma>,
}

impl Parse for AstInlineError {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        let save_position = input.fork();
        let _brace_token = braced!(content in input);
        let error_variants = content.parse_terminated(
            |input: ParseStream| input.parse::<AstErrorVariant>(),
            token::Comma,
        )?;
        if error_variants.is_empty() {
            return Err(syn::parse::Error::new(
                save_position.span(),
                "Inline error variants cannot be empty",
            ));
        }
        return Ok(AstInlineError { error_variants });
    }
}

#[derive(Clone)]
pub(crate) struct RefError {
    pub(crate) name: Ident,
    pub(crate) generic_refs: Vec<Ident>,
}

impl Parse for RefError {
    fn parse(input: ParseStream) -> Result<Self> {
        let name = input.parse::<Ident>()?;
        let generics = generics(&input)?;
        Ok(RefError {
            name,
            generic_refs: generics,
        })
    }
}

//************************************************************************//

/// A variant for an error
#[derive(Clone)]
pub(crate) struct AstErrorVariant {
    pub(crate) attributes: Vec<Attribute>,
    pub(crate) cfg_attributes: Vec<Attribute>,
    pub(crate) display: Option<DisplayAttribute>,
    pub(crate) name: Ident,
    // Dev Note: `Some(Vec::new())` == `{}`, `Some(Vec::new(..))` == `{..}`, `None` == ``. `{}` means inline struct if has source as well.
    pub(crate) fields: Option<Vec<AstInlineErrorVariantField>>,
    pub(crate) source_type: Option<syn::TypePath>,
    #[allow(dead_code)] // todo remove when this is implemented
    pub(crate) backtrace_type: Option<syn::TypePath>,
}

impl Parse for AstErrorVariant {
    fn parse(input: ParseStream) -> Result<Self> {
        let attributes = input.call(Attribute::parse_outer)?;
        let (mut attributes, cfg_attributes) = extract_cfg(attributes);
        let display = extract_display_attribute(&mut attributes)?;
        let name = input.parse::<Ident>()?;
        let content: syn::Result<_> = (|| {
            let content;
            parenthesized!(content in input);
            return Ok(content);
        })();
        let mut source_type = None;
        let mut backtrace_type = None;
        if let Ok(content) = content {
            let source_and_backtrace = content.parse_terminated(
                |input: ParseStream| input.parse::<syn::TypePath>(),
                token::Comma,
            );
            if let Ok(source_and_backtrace) = source_and_backtrace {
                if source_and_backtrace.len() <= 2 {
                    let mut source_and_backtrace = source_and_backtrace.into_iter();
                    source_type = source_and_backtrace.next();
                    backtrace_type = source_and_backtrace.next();
                } else {
                    return Err(syn::parse::Error::new(
                        source_and_backtrace.span(),
                        format!("Expected at most two elements - a source error type and a backtrace. Recieved {}.",source_and_backtrace.len() ),
                    ));
                }
            }
        }
        let content: syn::Result<_> = (|| {
            let content;
            syn::braced!(content in input);
            return Ok(content);
        })();
        let content = match content {
            Err(_) => {
                return Ok(AstErrorVariant {
                    attributes,
                    cfg_attributes,
                    display,
                    name,
                    fields: None,
                    source_type,
                    backtrace_type,
                });
            }
            Ok(content) => content,
        };
        let fields = content
            .parse_terminated(AstInlineErrorVariantField::parse, syn::Token![,])?
            .into_iter()
            .collect::<Vec<_>>();
        let fields = Some(fields);
        Ok(AstErrorVariant {
            attributes,
            cfg_attributes,
            display,
            name,
            fields,
            source_type,
            backtrace_type,
        })
    }
}

//************************************************************************//

fn generics<T: Parse>(input: &ParseStream) -> Result<Vec<T>> {
    if input.peek(syn::Token![<]) {
        input.parse::<syn::Token![<]>()?;
        let mut generics = Vec::new();
        loop {
            let next = input.parse::<T>();
            match next {
                Ok(next) => generics.push(next),
                Err(_) => {}
            }
            let punc = input.parse::<syn::Token![,]>();
            if punc.is_err() {
                break;
            }
        }
        input.parse::<syn::Token![>]>()?;
        Ok(generics)
    } else {
        Ok(Vec::new())
    }
}

//************************************************************************//

#[derive(Clone)]
pub(crate) struct DisableArg {
    pub(crate) name: Ident,
    pub(crate) refs: Vec<syn::TypePath>,
}

impl Parse for DisableArg {
    fn parse(input: ParseStream) -> Result<Self> {
        let name = input.parse::<Ident>()?;
        let content: syn::Result<_> = (|| {
            let content;
            parenthesized!(content in input);
            return Ok(content);
        })();
        let refs = if let Ok(content) = content {
            let refs = content
                .parse_terminated(
                    |input: ParseStream| input.parse::<syn::TypePath>(),
                    token::Comma,
                )
                .ok();
            if let Some(refs) = refs {
                refs.into_iter().collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(DisableArg { name, refs })
    }
}

fn extract_disabled(attributes: &mut Vec<Attribute>) -> syn::Result<Disabled> {
    let mut to_remove = Vec::new();
    let mut disabled = Disabled::default();
    for (i, e) in attributes.iter().enumerate() {
        let this_disabled = extract_disabled_helper(e)?;
        if let Some(this_disabled) = this_disabled {
            disabled.merge(this_disabled);
            to_remove.push(i);
        }
    }

    if to_remove.is_empty() {
        return Ok(disabled);
    }
    let mut index = 0;
    attributes.retain(|_| {
        let retain = !&to_remove.contains(&index);
        index += 1;
        return retain;
    });

    Ok(disabled)
}

fn extract_disabled_helper(attribute: &Attribute) -> syn::Result<Option<Disabled>> {
    return match &attribute.meta {
        syn::Meta::Path(_) => Ok(None),
        syn::Meta::NameValue(_) => Ok(None),
        syn::Meta::List(list) => {
            let ident = list.path.get_ident();
            let Some(ident) = ident else {
                return Ok(None);
            };
            let ident = ident.to_string();
            if &*ident != DISABLE_ATTRIBUTE_NAME {
                return Ok(None);
            }

            let punc = match syn::parse::Parser::parse2(
                &|input: ParseStream| {
                    Punctuated::<DisableArg, token::Comma>::parse_terminated(input)
                },
                list.tokens.clone(),
            ) {
                Ok(okay) => okay,
                Err(_) => {
                    return Err(syn::parse::Error::new(
                        list.tokens.span(),
                        format!("Invalid syntax for `{}` attribute.", DISABLE_ATTRIBUTE_NAME),
                    ))
                }
            };
            let mut from = None;
            let mut display = false;
            let mut debug = false;
            let mut error = false;
            for DisableArg { name, refs } in punc {
                let ident = name.to_string();
                match &*ident {
                    "From" => {
                        from = Some(refs);
                    }
                    "Display" => {
                        display = true;
                        if !refs.is_empty() {
                            return Err(syn::parse::Error::new(
                                name.span(),
                                format!(
                                    "`Display` does not take any arguments for `{}` attribute.",
                                    DISABLE_ATTRIBUTE_NAME
                                ),
                            ));
                        }
                    }
                    "Debug" => {
                        debug = true;
                        if !refs.is_empty() {
                            return Err(syn::parse::Error::new(
                                name.span(),
                                format!(
                                    "`Debug` does not take any arguments for `{}` attribute.",
                                    DISABLE_ATTRIBUTE_NAME
                                ),
                            ));
                        }
                    }
                    "Error" => {
                        error = true;
                        if !refs.is_empty() {
                            return Err(syn::parse::Error::new(
                                name.span(),
                                format!(
                                    "`Error` does not take any arguments for `{}` attribute.",
                                    DISABLE_ATTRIBUTE_NAME
                                ),
                            ));
                        }
                    }
                    _ => {
                        return Err(syn::parse::Error::new(
                            ident.span(),
                            format!(
                                "`{ident}` is not a valid option for `{DISABLE_ATTRIBUTE_NAME}`"
                            ),
                        ))
                    }
                }
            }
            Ok(Some(Disabled {
                from,
                display,
                debug,
                error,
            }))
        }
    };
}

#[derive(Clone)]
pub(crate) struct Disabled {
    /// `None` == no disabling, `Some` and empty == empty disables all, `Some` and args == only disable args
    pub(crate) from: Option<Vec<syn::TypePath>>,
    pub(crate) display: bool,
    pub(crate) debug: bool,
    pub(crate) error: bool,
}

impl Disabled {
    fn merge(&mut self, other: Disabled) {
        self.from = other.from;
        self.display = other.display;
        self.debug = other.debug;
        self.error = other.error;
    }
}

impl Default for Disabled {
    fn default() -> Self {
        Disabled {
            from: None,
            display: false,
            debug: false,
            error: false,
        }
    }
}

//************************************************************************//

/// The format string to use for display
#[derive(Clone)]
pub(crate) struct DisplayAttribute {
    pub(crate) tokens: TokenStream,
}

fn extract_display_attribute(
    attributes: &mut Vec<Attribute>,
) -> syn::Result<Option<DisplayAttribute>> {
    let mut to_remove = Vec::new();
    let mut displays = Vec::new();
    for (i, e) in attributes.iter().enumerate() {
        if let Some(display_tokens) = display_tokens(e) {
            displays.push(display_tokens);
            to_remove.push(i);
        }
    }
    if to_remove.is_empty() {
        return Ok(None);
    }
    let display = displays.remove(0);
    if to_remove.len() > 1 {
        return Err(syn::parse::Error::new(
            display.tokens.span(),
            format!("More than one `{}` attribute found", DISPLAY_ATTRIBUTE_NAME),
        ));
    }

    if to_remove.is_empty() {
        return Ok(Some(display));
    }
    let mut index = 0;
    attributes.retain(|_| {
        let retain = !&to_remove.contains(&index);
        index += 1;
        return retain;
    });
    Ok(Some(display))
}

fn display_tokens(attribute: &Attribute) -> Option<DisplayAttribute> {
    return match &attribute.meta {
        syn::Meta::Path(_) => None,
        syn::Meta::NameValue(_) => None,
        syn::Meta::List(list) => {
            let ident = list.path.get_ident();
            let Some(ident) = ident else {
                return None;
            };
            let ident = ident.to_string();
            if &*ident == DISPLAY_ATTRIBUTE_NAME {
                return Some(DisplayAttribute {
                    tokens: list.tokens.clone(),
                });
            }
            return None;
        }
    };
}

/// old and new
fn extract_cfg(attributes: Vec<Attribute>) -> (Vec<Attribute>, Vec<Attribute>) {
    let mut to_remove = Vec::new();
    for (index, attribute) in attributes.iter().enumerate() {
        match &attribute.meta {
            syn::Meta::NameValue(_) => {}
            syn::Meta::Path(_) => {}
            syn::Meta::List(meta_list) => {
                if meta_list
                    .path
                    .get_ident()
                    .is_some_and(|e| e.to_string() == "cfg")
                {
                    to_remove.push(index);
                }
            }
        };
    }

    let mut cfgs = Vec::new();
    let mut index: usize = usize::MAX;
    let attributes = attributes
        .into_iter()
        .filter_map(|e| {
            index = index.wrapping_add(1);
            if to_remove.contains(&index) {
                cfgs.push(e);
                return None;
            }
            Some(e)
        })
        .collect::<Vec<_>>();
    (attributes, cfgs)
}

#[derive(Clone, PartialEq)]
pub(crate) struct AstInlineErrorVariantField {
    pub(crate) name: Ident,
    pub(crate) r#type: syn::Type,
}

impl Parse for AstInlineErrorVariantField {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        let _: syn::Token![:] = input.parse()?;
        let r#type: syn::Type = input.parse()?;
        Ok(AstInlineErrorVariantField { name, r#type })
    }
}

impl Eq for AstInlineErrorVariantField {}

//************************************************************************//

fn last_token_span(input: ParseBuffer) -> proc_macro2::Span {
    let last_token = input.cursor().token_stream().into_iter().last();
    let Some(last_token) = last_token else {
        return proc_macro2::Span::call_site();
    };
    match last_token {
        proc_macro2::TokenTree::Group(group) => group.span_close(),
        proc_macro2::TokenTree::Ident(ident) => ident.span(),
        proc_macro2::TokenTree::Punct(punct) => punct.span(),
        proc_macro2::TokenTree::Literal(literal) => literal.span(),
    }
}
