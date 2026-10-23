//! `enum_with_names!`: an enum of unit and struct variants, written with its `VARIANT_NAMES` and
//! `variant_name` from one list. See [`enum_with_names`](crate::open_enum::enum_with_names).

/// Writes an enum of unit and struct variants with `VARIANT_NAMES` and `variant_name`, from one
/// list, so `VARIANT_NAMES` cannot miss a variant.
///
/// Nothing is derived: the caller writes the derives. The forms that pass through are doc
/// comments and `#[derive(Name, ..)]`, on the enum; `#[serde(rename_all = "..")]` and
/// `#[cfg_attr(feature = "specta", derive(path))]` on the enum; doc comments on a variant; and,
/// on a field, doc comments, `#[serde(key, key = "..", ..)]` and
/// `#[cfg_attr(feature = "specta", specta(type = ..))]`.
macro_rules! enum_with_names {
    (
        $(#[doc = $doc:expr])*
        $(#[derive($($derive:ident),* $(,)?)])*
        $(#[serde(rename_all = $rename:literal)])*
        $(#[cfg_attr(feature = "specta", derive($specta:path))])*
        $vis:vis enum $name:ident {
            $(
                $(#[doc = $variant_doc:expr])*
                $variant:ident $({
                    $(
                        $(#[doc = $field_doc:expr])*
                        $(#[serde($($serde_key:ident $(= $serde_value:literal)?),+)])*
                        $(#[cfg_attr(feature = "specta", specta(type = $spec:ty))])?
                        $field:ident: $ty:ty
                    ),+ $(,)?
                })?
            ),+ $(,)?
        }
    ) => {
        $(#[doc = $doc])*
        $(#[derive($($derive),*)])*
        $(#[serde(rename_all = $rename)])*
        $(#[cfg_attr(feature = "specta", derive($specta))])*
        $vis enum $name {
            $(
                $(#[doc = $variant_doc])*
                $variant $({
                    $(
                        $(#[doc = $field_doc])*
                        $(#[serde($($serde_key $(= $serde_value)?),+)])*
                        $(#[cfg_attr(feature = "specta", specta(type = $spec))])?
                        $field: $ty
                    ),+
                })?,
            )+
        }

        impl $name {
            /// The Rust name of every variant, in declaration order.
            $vis const VARIANT_NAMES: [&'static str; $crate::open_enum::count_idents!(
                $($variant)+
            )] = [$(stringify!($variant),)+];

            /// The Rust name of the variant, such as `"Queued"`, for tests.
            #[doc(hidden)]
            $vis const fn variant_name(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => stringify!($variant),)+
                }
            }
        }
    };
}
pub(crate) use enum_with_names;
