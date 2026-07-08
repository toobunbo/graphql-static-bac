use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TypeKind {
    Object,
    Interface,
    Union,
    InputObject,
    Enum,
    Scalar,
}

impl TypeKind {
    pub fn is_output_type(self) -> bool {
        matches!(
            self,
            Self::Object | Self::Interface | Self::Union | Self::Enum | Self::Scalar
        )
    }

    pub fn is_input_type(self) -> bool {
        matches!(self, Self::InputObject | Self::Enum | Self::Scalar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TypeWrapper {
    NonNull,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRef {
    pub display: String,
    pub named_type: String,
    pub named_kind: TypeKind,
    pub wrappers: Vec<TypeWrapper>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TypeRefError {
    #[error("NON_NULL cannot wrap another NON_NULL")]
    DoubleNonNull,
}

impl TypeRef {
    pub fn new(
        named_type: impl Into<String>,
        named_kind: TypeKind,
        wrappers: Vec<TypeWrapper>,
    ) -> Result<Self, TypeRefError> {
        let named_type = named_type.into();
        validate_wrappers(&wrappers)?;
        let display = render_type_ref(&named_type, &wrappers);
        Ok(Self {
            display,
            named_type,
            named_kind,
            wrappers,
        })
    }

    pub fn validate(&self) -> Result<(), TypeRefError> {
        validate_wrappers(&self.wrappers)
    }

    pub fn rendered(&self) -> String {
        render_type_ref(&self.named_type, &self.wrappers)
    }
}

fn validate_wrappers(wrappers: &[TypeWrapper]) -> Result<(), TypeRefError> {
    for pair in wrappers.windows(2) {
        if pair == [TypeWrapper::NonNull, TypeWrapper::NonNull] {
            return Err(TypeRefError::DoubleNonNull);
        }
    }
    Ok(())
}

fn render_type_ref(named_type: &str, wrappers: &[TypeWrapper]) -> String {
    let mut rendered = named_type.to_string();
    for wrapper in wrappers.iter().rev() {
        match wrapper {
            TypeWrapper::NonNull => rendered.push('!'),
            TypeWrapper::List => rendered = format!("[{rendered}]"),
        }
    }
    rendered
}
