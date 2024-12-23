use std::collections::HashMap;

use crate::ast::{
    AstErrorDeclaration, AstErrorSet, AstErrorVariant, AstInlineErrorVariantField, Disabled, RefError
};
use crate::expand::{ErrorEnum, ErrorVariant, Named, SourceStruct, SourceTuple, Struct};

use syn::{Attribute, Ident, TypeParam};

/// Constructs [ErrorEnum]s from the ast, resolving any references to other sets. The returned result is
/// all error sets with the full expansion.
pub(crate) fn resolve(error_set: AstErrorSet) -> syn::Result<(Vec<ErrorEnum>, Vec<Ident>)> {
    let mut error_enum_builders: Vec<ErrorEnumBuilder> = Vec::new();
    for declaration in error_set.set_items.into_iter() {
        let AstErrorDeclaration {
            attributes,
            error_name,
            generics,
            disabled,
            parts,
        } = declaration;

        let mut error_enum_builder = ErrorEnumBuilder::new(error_name, attributes, generics, disabled);

        for part in parts.into_iter() {
            match part {
                crate::ast::AstInlineOrRefError::Inline(inline_part) => {
                    error_enum_builder
                        .error_variants
                        .extend(inline_part.error_variants.into_iter());
                }
                crate::ast::AstInlineOrRefError::Ref(ref_part) => {
                    error_enum_builder.add_ref_part(ref_part);
                }
            }
        }
        error_enum_builders.push(error_enum_builder);
    }
    
    // let mut x = ref_part.name.clone();
    // let span = x.span().resolved_at(error_enum_builder.error_name.span());
    // x.set_span(span);
    // all_ref_parts.push(x);
    let mut all_ref_parts = error_enum_builders.iter().map(|e| &e.ref_parts_to_resolve).flatten().map(|e| 
    {
        e.name.clone()
        
    }).collect::<Vec<_>>();
    for part in all_ref_parts.iter_mut() {
        for error_enum_builder in error_enum_builders.iter() {
            if error_enum_builder.error_name == *part {
                // return Err(syn::parse::Error::new_spanned(
                //     part.clone(),
                //     "asgsgsgf.",
                // ));
                // part.set_span(part.span().located_at(error_enum_builder.error_name.span()));
                // part.set_span(part.span().resolved_at(error_enum_builder.error_name.span()));
                // part.set_span(error_enum_builder.error_name.span().located_at(part.span()));
                // part.set_span(error_enum_builder.error_name.span().resolved_at(part.span()));

                // part.set_span(part.span().join(error_enum_builder.error_name.span()).unwrap());

                // let span = error_enum_builder.error_name.span();
                // span.unwrap().end()
            }
        }
    }
    let error_enums = resolve_builders(error_enum_builders)?;

    Ok((error_enums, all_ref_parts))
}

fn resolve_builders(mut error_enum_builders: Vec<ErrorEnumBuilder>) -> syn::Result<Vec<ErrorEnum>> {
    for index in 0..error_enum_builders.len() {
        if !error_enum_builders[index].ref_parts_to_resolve.is_empty() {
            resolve_builders_helper(index, &mut *error_enum_builders, &mut Vec::new())?;
        }
    }
    let error_enums = error_enum_builders
        .into_iter()
        .map(Into::into)
        .collect::<Vec<ErrorEnum>>();
    Ok(error_enums)
}

fn resolve_builders_helper<'a>(
    index: usize,
    error_enum_builders: &'a mut [ErrorEnumBuilder],
    visited: &mut Vec<Ident>,
) -> syn::Result<Vec<AstErrorVariant>> {
    //println!("visited `{}`", visited.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(" - "));
    let error_enum_builder = &error_enum_builders[index];
    let error_name = &error_enum_builder.error_name;
    if visited.contains(error_name) {
        visited.push(error_name.clone());
        if let Some(pos) = visited.iter().position(|e| e == error_name) {
            visited.drain(0..pos);
        }
        return Err(syn::parse::Error::new_spanned(
            error_name.clone(),
            format!(
                "Cycle Detected: {}",
                visited
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("->")
            ),
        ));
    }
    let ref_parts_to_resolve = error_enum_builder.ref_parts_to_resolve.clone();
    // If this enums ref parts have not been resolved, resolve them.
    if !ref_parts_to_resolve.is_empty() {
        for ref_part in ref_parts_to_resolve {
            let ref_error_enum_index = error_enum_builders
                .iter()
                .position(|e| e.error_name == ref_part.name);
            let ref_error_enum_index = match ref_error_enum_index {
                Some(e) => e,
                None => {
                    return Err(syn::parse::Error::new_spanned(
                        &ref_part.name,
                        "Not a declared error set.",
                    ));
                }
            };
            if !error_enum_builders[ref_error_enum_index]
                .ref_parts_to_resolve
                .is_empty()
            {
                visited.push(error_enum_builders[index].error_name.clone());
                resolve_builders_helper(ref_error_enum_index, error_enum_builders, visited)?;
                visited.pop();
            }
            let (this_error_enum_builder, ref_error_enum_builder) =
                indices::indices!(&mut *error_enum_builders, index, ref_error_enum_index);
            // Let the ref declaration override the original generic declaration name to avoid collisions - `.. || X<T> ..`
            if ref_part.generic_refs.len() != ref_error_enum_builder.generics.len() {
                Err(syn::parse::Error::new_spanned(
                    &ref_part.name,
                    format!("A reference to {} was declared with {} generic param(s), but the original definition takes {}.", ref_part.name, ref_part.generic_refs.len(), ref_error_enum_builder.generics.len()),
                ))?;
            }
            let mut error_variants = Vec::new();
            let error_variants = if ref_part.generic_refs.is_empty() {
                &ref_error_enum_builder.error_variants
            } else {
                fn ident_to_type(ident: Ident) -> syn::Type {
                    let segment = syn::PathSegment {
                        ident,
                        arguments: syn::PathArguments::None, // No generic arguments
                    };
                    let path = syn::Path {
                        leading_colon: None,
                        segments: {
                            let mut punctuated = syn::punctuated::Punctuated::new();
                            punctuated.push(segment);
                            punctuated
                        },
                    };
                    let type_path = syn::TypePath { qself: None, path };
                    syn::Type::Path(type_path)
                }
                // rename the generics inside the variants to the new declared name - to avoid collisions.
                let mut rename = HashMap::<syn::Type, syn::Type>::new();
                for (ref_part_generic, ref_error_enum_generic) in ref_part
                    .generic_refs
                    .iter()
                    .zip(ref_error_enum_builder.generics.iter())
                {
                    rename.insert(
                        ident_to_type(ref_error_enum_generic.ident.clone()),
                        ident_to_type(ref_part_generic.clone()),
                    );
                }
                // let error_variants = Vec::new();
                for error_variant in ref_error_enum_builder.error_variants.iter() {
                    let new_fields = if let Some(fields) = &error_variant.fields {
                        let mut new_fields = Vec::new();
                        for field in fields.iter() {
                            if rename.contains_key(&field.r#type) {
                                let new_type = rename.get(&field.r#type).unwrap().clone();
                                new_fields.push(AstInlineErrorVariantField {
                                    name: field.name.clone(),
                                    r#type: new_type.clone(),
                                });
                            } else {
                                new_fields.push(field.clone());
                            }
                        }
                        Some(new_fields)
                    } else {
                        None
                    };
                    error_variants.push(AstErrorVariant {
                        attributes: error_variant.attributes.clone(),
                        cfg_attributes: error_variant.cfg_attributes.clone(),
                        display: error_variant.display.clone(),
                        name: error_variant.name.clone(),
                        fields: new_fields,
                        source_type: error_variant.source_type.clone(),
                        backtrace_type: error_variant.backtrace_type.clone(),
                    });
                }
                &error_variants
            };
            for variant in error_variants {
                let this_error_variants = &mut this_error_enum_builder.error_variants;
                let is_variant_already_in_enum = this_error_variants
                    .iter()
                    .any(|e| does_occupy_the_same_space(e, &variant));
                if !is_variant_already_in_enum {
                    this_error_variants.push(variant.clone());
                }
            }
        }
        error_enum_builders[index].ref_parts_to_resolve.clear();
    }
    // Now that are refs are solved and included in this error_enum_builder's error_variants, return them.
    Ok(error_enum_builders[index].error_variants.clone())
}

/// If the error definitions occupy the same space. Useful since if this space is already occupied e.g. ` X = A || B`
/// If `A` has a variant like `V1(std::io::Error)` and `B` `V1(std::io::Error)`.
pub(crate) fn does_occupy_the_same_space(this: &AstErrorVariant, other: &AstErrorVariant) -> bool {
    return this.name == other.name;
}

// fn merge_generics(this: &mut Generics, other: &Generics) {
//     let other_params = other.params.iter().collect::<Vec<_>>();
//     for other_param in other_params {
//         if !this.params.iter().any(|param| param == other_param) {
//             this.params.push(other_param.clone());
//         }
//     }
//     let other_where = other.where_clause.as_ref();
//     if let Some(other_where) = other_where {
//         if let Some(this_where) = &mut this.where_clause {
//             this_where.predicates.extend(other_where.predicates.clone());
//         } else {
//             this.where_clause = Some(other_where.clone());
//         }
//     }
// }

struct ErrorEnumBuilder {
    pub attributes: Vec<Attribute>,
    pub error_name: Ident,
    pub generics: Vec<TypeParam>,
    pub disabled: Disabled,
    pub error_variants: Vec<AstErrorVariant>,
    /// Once this is empty, all [ref_parts] have been resolved and [error_variants] is complete.
    pub ref_parts_to_resolve: Vec<RefError>,
}

impl ErrorEnumBuilder {
    fn new(error_name: Ident, attributes: Vec<Attribute>, generics: Vec<TypeParam>, disabled: Disabled) -> Self {
        Self {
            attributes,
            error_name,
            generics,
            disabled,
            error_variants: Vec::new(),
            ref_parts_to_resolve: Vec::new(),
        }
    }

    fn add_ref_part(&mut self, ref_part: RefError) {
        self.ref_parts_to_resolve.push(ref_part);
    }
}

impl From<ErrorEnumBuilder> for ErrorEnum {
    fn from(value: ErrorEnumBuilder) -> Self {
        assert!(
            value.ref_parts_to_resolve.is_empty(),
            "All references should be resolved when converting to an error enum."
        );
        ErrorEnum {
            attributes: value.attributes,
            error_name: value.error_name,
            generics: value.generics,
            disabled: value.disabled,
            error_variants: value
                .error_variants
                .into_iter()
                .map(|v| reshape(v))
                .collect::<Vec<_>>(),
        }
    }
}

impl PartialEq for ErrorEnumBuilder {
    fn eq(&self, other: &Self) -> bool {
        self.error_name == other.error_name
    }
}

impl std::hash::Hash for ErrorEnumBuilder {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.error_name.hash(state);
    }
}

impl Eq for ErrorEnumBuilder {}

//************************************************************************//

fn reshape(this: AstErrorVariant) -> ErrorVariant {
    let AstErrorVariant {
        attributes,
        cfg_attributes,
        display,
        name,
        fields,
        source_type,
        backtrace_type: _,
    } = this;
    match (fields, source_type) {
        // e.g. `Variant(std::io::Error) {}` or `Variant(std::io::Error) {...}`
        (Some(fields), Some(source_type)) => {
            return ErrorVariant::SourceStruct(SourceStruct {
                attributes,
                cfg_attributes,
                display,
                name,
                source_type,
                fields,
            });
        }
        // e.g. `Variant(std::io::Error)`
        (Some(fields), None) => {
            return ErrorVariant::Struct(Struct {
                attributes,
                cfg_attributes,
                display,
                name,
                fields,
            });
        }
        // e.g. `Variant(std::io::Error)`
        (None, Some(source_type)) => {
            return ErrorVariant::SourceTuple(SourceTuple {
                attributes,
                cfg_attributes,
                display,
                name,
                source_type,
            });
        }
        // e.g. `Variant {}`
        (None, None) => {
            return ErrorVariant::Named(Named {
                attributes,
                cfg_attributes,
                display,
                name,
            });
        }
    }
}
