//! The walk over syn's tree, and the item each site names.
//!
//! A site names the item its attribute sits on, as a path from the top of the file joined with
//! `::`: a function `f`, a field `Entry::replacement`, a method `A::fmt` (an impl is named by the
//! last segment of its type, with no generics), a trait default method `T::m`, an item in a module
//! `m::f`, a variant `E::V`, a tuple field `S::0`. An attribute on a statement, an expression, a
//! parameter or a generic names the nearest item around it, and so does one found in a macro's
//! tokens. At the top level of the file, an attribute on no named item (an inner attribute of the
//! file, a `use`, an `extern` block, a macro call) names `-`.

use super::{Finder, Origin};
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

/// The name of an identifier, without `r#`.
fn name(ident: &syn::Ident) -> String {
    ident.unraw().to_string()
}

/// The name of an impl's type: the last segment of its path, with no generics, through
/// references, pointers, slices, arrays and parentheses; `_` for any other type.
fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map_or_else(|| "_".to_owned(), |segment| name(&segment.ident)),
        syn::Type::Reference(inner) => type_name(&inner.elem),
        syn::Type::Ptr(inner) => type_name(&inner.elem),
        syn::Type::Slice(inner) => type_name(&inner.elem),
        syn::Type::Array(inner) => type_name(&inner.elem),
        syn::Type::Paren(inner) => type_name(&inner.elem),
        syn::Type::Group(inner) => type_name(&inner.elem),
        _ => "_".to_owned(),
    }
}

/// One `Visit` method per kind of named item: push its name, then walk it as syn does.
macro_rules! named {
    ($($method:ident($node:ty) => |$i:ident| $name:expr;)*) => {$(
        fn $method(&mut self, $i: &'ast $node) {
            self.named($name, |finder| visit::$method(finder, $i));
        }
    )*};
}

impl<'ast> Visit<'ast> for Finder {
    fn visit_attribute(&mut self, i: &'ast syn::Attribute) {
        self.attribute(i, Origin::Tree { resolve: false });
    }

    named! {
        visit_item_fn(syn::ItemFn) => |i| name(&i.sig.ident);
        visit_item_const(syn::ItemConst) => |i| name(&i.ident);
        visit_item_static(syn::ItemStatic) => |i| name(&i.ident);
        visit_item_struct(syn::ItemStruct) => |i| name(&i.ident);
        visit_item_enum(syn::ItemEnum) => |i| name(&i.ident);
        visit_item_union(syn::ItemUnion) => |i| name(&i.ident);
        visit_item_trait(syn::ItemTrait) => |i| name(&i.ident);
        visit_item_trait_alias(syn::ItemTraitAlias) => |i| name(&i.ident);
        visit_item_type(syn::ItemType) => |i| name(&i.ident);
        visit_item_extern_crate(syn::ItemExternCrate) => |i| name(&i.ident);
        visit_item_impl(syn::ItemImpl) => |i| type_name(&i.self_ty);
        visit_impl_item_fn(syn::ImplItemFn) => |i| name(&i.sig.ident);
        visit_impl_item_const(syn::ImplItemConst) => |i| name(&i.ident);
        visit_impl_item_type(syn::ImplItemType) => |i| name(&i.ident);
        visit_trait_item_fn(syn::TraitItemFn) => |i| name(&i.sig.ident);
        visit_trait_item_const(syn::TraitItemConst) => |i| name(&i.ident);
        visit_trait_item_type(syn::TraitItemType) => |i| name(&i.ident);
        visit_foreign_item_fn(syn::ForeignItemFn) => |i| name(&i.sig.ident);
        visit_foreign_item_static(syn::ForeignItemStatic) => |i| name(&i.ident);
        visit_foreign_item_type(syn::ForeignItemType) => |i| name(&i.ident);
        visit_variant(syn::Variant) => |i| name(&i.ident);
    }

    /// A module at the top level of the file resolves its `#[path]` against the file's folder.
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        let resolve = self.path.is_empty();
        self.named(name(&i.ident), |finder| {
            for attr in &i.attrs {
                finder.attribute(attr, Origin::Tree { resolve });
            }
            for item in i.content.iter().flat_map(|(_, items)| items) {
                finder.visit_item(item);
            }
        });
    }

    /// A `macro_rules!` definition names its macro; a macro call at the top level names nothing.
    fn visit_item_macro(&mut self, i: &'ast syn::ItemMacro) {
        match &i.ident {
            Some(ident) => self.named(name(ident), |finder| {
                visit::visit_item_macro(finder, i);
            }),
            None => visit::visit_item_macro(self, i),
        }
    }

    fn visit_field(&mut self, i: &'ast syn::Field) {
        match &i.ident {
            Some(ident) => self.named(name(ident), |finder| visit::visit_field(finder, i)),
            None => visit::visit_field(self, i),
        }
    }

    /// A tuple field is named by its index.
    fn visit_fields_unnamed(&mut self, i: &'ast syn::FieldsUnnamed) {
        for (index, field) in i.unnamed.iter().enumerate() {
            self.named(index.to_string(), |finder| finder.visit_field(field));
        }
    }

    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        if let Some(last) = i.path.segments.last()
            && last.ident == "include"
        {
            self.include(i.tokens.clone(), last.ident.span());
        }
        self.tokens(i.tokens.clone());
        visit::visit_macro(self, i);
    }
}
