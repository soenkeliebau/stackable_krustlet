use std::collections::HashMap;
use std::convert::TryFrom;
use k8s_openapi::serde_value::DeserializerError;
use serde::{Deserialize, Serialize};
use crate::error::StackableError::PackageParseError;
use oci_distribution::Reference;
use crate::error::StackableError;
use std::fmt;


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Package {
    pub product: String,
    pub version: String,
}

impl TryFrom<Reference> for Package {
    type Error = StackableError;

    fn try_from(value: Reference) -> Result<Self, Self::Error> {
        Ok(Package {
            product: String::from(value.repository()),
            version: String::from(value.tag().unwrap()),
        })
    }
}
impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.product, self.version)
    }
}

