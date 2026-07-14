use serde_json::Value;
use thiserror::Error;

use crate::catalog::{ALLOWED_COMPONENTS, ASTRO_CATALOG_ID};

const OP_KEYS: &[&str] = &[
    "createSurface",
    "updateComponents",
    "updateDataModel",
    "deleteSurface",
];

#[derive(Debug, Error)]
pub enum Error {
    #[error("operation missing version")]
    MissingVersion,
    #[error("operation must have exactly one of: createSurface, updateComponents, updateDataModel, deleteSurface")]
    InvalidOperationKind,
    #[error("createSurface.catalogId must be {expected}, got {got:?}")]
    InvalidCatalogId { expected: &'static str, got: Option<String> },
    #[error("unknown component: {0}")]
    UnknownComponent(String),
    #[error("invalid operation structure: {0}")]
    InvalidStructure(&'static str),
}

pub fn validate_operations(ops: &[Value]) -> Result<(), Error> {
    for op in ops {
        let obj = op
            .as_object()
            .ok_or(Error::InvalidStructure("operation must be an object"))?;

        if !obj.contains_key("version") {
            return Err(Error::MissingVersion);
        }

        let kind_count = OP_KEYS.iter().filter(|k| obj.contains_key(**k)).count();
        if kind_count != 1 {
            return Err(Error::InvalidOperationKind);
        }

        if let Some(create) = obj.get("createSurface") {
            let catalog_id = create
                .get("catalogId")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            if catalog_id.as_deref() != Some(ASTRO_CATALOG_ID) {
                return Err(Error::InvalidCatalogId {
                    expected: ASTRO_CATALOG_ID,
                    got: catalog_id,
                });
            }
        }

        if let Some(update) = obj.get("updateComponents") {
            let components = update
                .get("components")
                .and_then(|v| v.as_array())
                .ok_or(Error::InvalidStructure(
                    "updateComponents.components must be an array",
                ))?;
            for component in components {
                let name = component
                    .get("component")
                    .and_then(|v| v.as_str())
                    .ok_or(Error::InvalidStructure(
                        "component entry must have a string component field",
                    ))?;
                if !ALLOWED_COMPONENTS.contains(&name) {
                    return Err(Error::UnknownComponent(name.to_owned()));
                }
            }
        }

        if let Some(delete) = obj.get("deleteSurface") {
            let sid = delete.get("surfaceId").and_then(|v| v.as_str());
            if sid.map(|s| s.is_empty()).unwrap_or(true) {
                return Err(Error::InvalidStructure(
                    "deleteSurface.surfaceId must be a non-empty string",
                ));
            }
        }
    }
    Ok(())
}
