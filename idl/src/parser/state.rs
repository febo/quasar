//! Parses `#[account]` state structs for IDL generation (field types,
//! discriminators, dynamic layout classification).

use {
    super::helpers,
    crate::types::{IdlAccountDef, IdlField, IdlTypeDef, IdlTypeDefType},
    syn::{Fields, Item},
};

/// Raw parsed data for a `#[account(discriminator = N)]` struct.
pub struct RawStateAccount {
    pub name: String,
    pub discriminator: Vec<u8>,
    pub fields: Vec<(String, syn::Type)>,
    pub seeds: Option<RawTypedSeeds>,
}

/// Parsed `#[seeds(b"prefix", name: Type, ...)]` on a state type.
pub struct RawTypedSeeds {
    pub prefix: Vec<u8>,
    pub dynamic_seeds: Vec<(String, String)>, // (name, type_name)
}

/// Extract all `#[account(discriminator = N)]` structs from a parsed file.
pub fn extract_state_accounts(file: &syn::File) -> Vec<RawStateAccount> {
    let mut result = Vec::new();
    for item in &file.items {
        if let Item::Struct(item_struct) = item {
            if let Some(disc) = get_account_discriminator(&item_struct.attrs) {
                let name = item_struct.ident.to_string();
                let fields = match &item_struct.fields {
                    Fields::Named(named) => named
                        .named
                        .iter()
                        .map(|f| {
                            let field_name = f.ident.as_ref().unwrap().to_string();
                            (field_name, f.ty.clone())
                        })
                        .collect(),
                    _ => vec![],
                };

                let seeds = parse_seeds_attr(&item_struct.attrs);

                result.push(RawStateAccount {
                    name,
                    discriminator: disc,
                    fields,
                    seeds,
                });
            }
        }
    }
    result
}

/// Check if a struct has `#[account(discriminator = N)]` and extract the
/// discriminator. Distinguishes from `#[account(...)]` field attributes on
/// derive(Accounts) fields by checking if it's on a struct item (not a field).
fn get_account_discriminator(attrs: &[syn::Attribute]) -> Option<Vec<u8>> {
    for attr in attrs {
        if !attr.path().is_ident("account") {
            continue;
        }

        let tokens = match attr.meta.require_list() {
            Ok(list) => list.tokens.to_string(),
            Err(_) => continue,
        };

        if !tokens.contains("discriminator") {
            continue;
        }

        return helpers::parse_discriminator_value(&tokens);
    }
    None
}

/// Parse `#[seeds(b"prefix", name: Type, ...)]` from struct attributes.
fn parse_seeds_attr(attrs: &[syn::Attribute]) -> Option<RawTypedSeeds> {
    for attr in attrs {
        if !attr.path().is_ident("seeds") {
            continue;
        }

        let tokens_str = match attr.meta.require_list() {
            Ok(list) => list.tokens.to_string(),
            Err(_) => continue,
        };

        // Parse the prefix (first element: byte string literal)
        let prefix = parse_seed_prefix(&tokens_str)?;

        // Parse dynamic seeds (remaining `name: Type` pairs)
        let dynamic_seeds = parse_dynamic_seeds(&tokens_str);

        return Some(RawTypedSeeds {
            prefix,
            dynamic_seeds,
        });
    }
    None
}

/// Extract the byte string prefix from a seeds attribute token string.
/// e.g. `b"escrow" , maker : Address` → b"escrow"
fn parse_seed_prefix(tokens_str: &str) -> Option<Vec<u8>> {
    let trimmed = tokens_str.trim();
    // Find b"..."
    let start = trimmed.find("b\"")?;
    let after_b_quote = &trimmed[start + 2..];
    let end_quote = after_b_quote.find('"')?;
    let prefix_str = &after_b_quote[..end_quote];
    Some(prefix_str.as_bytes().to_vec())
}

/// Extract dynamic seed `name: Type` pairs from a seeds token string.
/// e.g. `b"escrow" , maker : Address` → [("maker", "Address")]
fn parse_dynamic_seeds(tokens_str: &str) -> Vec<(String, String)> {
    let trimmed = tokens_str.trim();
    // Find the end of the byte string prefix
    let start = match trimmed.find("b\"") {
        Some(s) => s,
        None => return vec![],
    };
    let after_b_quote = &trimmed[start + 2..];
    let end_quote = match after_b_quote.find('"') {
        Some(e) => e,
        None => return vec![],
    };
    // Skip past the prefix and the closing quote
    let after_prefix = &after_b_quote[end_quote + 1..];

    let mut result = Vec::new();
    // Split remaining by comma and parse `name : Type` pairs
    for part in after_prefix.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(colon_idx) = part.find(':') {
            let name = part[..colon_idx].trim().to_string();
            let ty = part[colon_idx + 1..].trim().to_string();
            if !name.is_empty() && !ty.is_empty() {
                result.push((name, ty));
            }
        }
    }
    result
}

/// Convert a `RawStateAccount` to an `IdlAccountDef` (for the "accounts"
/// array).
pub fn to_idl_account_def(raw: &RawStateAccount) -> IdlAccountDef {
    IdlAccountDef {
        name: raw.name.clone(),
        discriminator: raw.discriminator.clone(),
    }
}

/// Convert a `RawStateAccount` to an `IdlTypeDef` (for the "types" array).
pub fn to_idl_type_def(raw: &RawStateAccount) -> IdlTypeDef {
    let fields = raw
        .fields
        .iter()
        .map(|(name, ty)| IdlField {
            name: helpers::to_camel_case(name),
            ty: helpers::map_type_from_syn(ty),
        })
        .collect();

    IdlTypeDef {
        name: raw.name.clone(),
        ty: IdlTypeDefType {
            kind: "struct".to_string(),
            fields,
        },
    }
}
