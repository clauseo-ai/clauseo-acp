//! What `Self::X` and `T::X` name where they are written, for the core-io check: the associated
//! types the enclosing impl or trait sets, the methods an inherent impl defines, and the
//! associated types the bounds and `where` clauses in scope pin for a type parameter.

use super::names::{self, Written};
use std::collections::{BTreeMap, BTreeSet};

/// What `Self` means inside one impl or trait.
#[derive(Debug, Default)]
pub(super) struct SelfScope {
    /// The impl's self type, when it is a path (`None` in a trait).
    pub self_type: Option<Written>,
    /// The associated types it sets: `type X = <path>`, `Self` already replaced.
    pub types: BTreeMap<String, Written>,
    /// The methods of an inherent impl: there `Self::<method>` is that method.
    pub methods: BTreeSet<String>,
}

/// The scope of `Self` in `item`.
pub(super) fn impl_scope(item: &syn::ItemImpl) -> SelfScope {
    let self_type = names::impl_self_type(item);
    let mut scope = SelfScope::default();
    for member in &item.items {
        match member {
            syn::ImplItem::Type(ty) => {
                if let Some(path) = names::type_path(&ty.ty) {
                    let path = names::with_self(names::written(path), self_type.as_ref());
                    scope.types.insert(names::name_of(&ty.ident), path);
                }
            }
            syn::ImplItem::Fn(method) if item.trait_.is_none() => {
                scope.methods.insert(names::name_of(&method.sig.ident));
            }
            _ => {}
        }
    }
    scope.self_type = self_type;
    scope
}

/// The scope of `Self` in `item`: the defaults of its associated types.
pub(super) fn trait_scope(item: &syn::ItemTrait) -> SelfScope {
    let mut scope = SelfScope::default();
    for member in &item.items {
        if let syn::TraitItem::Type(ty) = member
            && let Some((_, default)) = &ty.default
            && let Some(path) = names::type_path(default)
        {
            scope
                .types
                .insert(names::name_of(&ty.ident), names::written(path));
        }
    }
    scope
}

/// One type parameter brought into scope, or one named by a `where` clause, with the
/// associated types its bounds there pin (`T: K<C = X>` pins `C` to `X`).
#[derive(Debug)]
pub(super) struct Param {
    pub name: String,
    pub types: BTreeMap<String, Vec<Written>>,
}

/// The type parameters of `generics` and the parameters its `where` clause bounds.
pub(super) fn params(generics: &syn::Generics) -> Vec<Param> {
    let mut found: Vec<Param> = generics
        .type_params()
        .map(|param| Param {
            name: names::name_of(&param.ident),
            types: pinned(&param.bounds),
        })
        .collect();
    let predicates = generics
        .where_clause
        .iter()
        .flat_map(|clause| &clause.predicates);
    for predicate in predicates {
        let syn::WherePredicate::Type(predicate) = predicate else {
            continue;
        };
        let Some(ident) = names::type_path(&predicate.bounded_ty).and_then(|path| path.get_ident())
        else {
            continue;
        };
        found.push(Param {
            name: names::name_of(ident),
            types: pinned(&predicate.bounds),
        });
    }
    found
}

/// The associated types `bounds` pin, by name.
fn pinned<'a>(
    bounds: impl IntoIterator<Item = &'a syn::TypeParamBound>,
) -> BTreeMap<String, Vec<Written>> {
    let mut types: BTreeMap<String, Vec<Written>> = BTreeMap::new();
    for bound in bounds {
        let syn::TypeParamBound::Trait(bound) = bound else {
            continue;
        };
        let Some(syn::PathArguments::AngleBracketed(args)) =
            bound.path.segments.last().map(|segment| &segment.arguments)
        else {
            continue;
        };
        for arg in &args.args {
            if let syn::GenericArgument::AssocType(pin) = arg
                && let Some(path) = names::type_path(&pin.ty)
            {
                let name = names::name_of(&pin.ident);
                types.entry(name).or_default().push(names::written(path));
            }
        }
    }
    types
}
