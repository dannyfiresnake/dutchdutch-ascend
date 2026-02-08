use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// API request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub meta: RequestMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Request metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMeta {
    pub id: Uuid,
    pub endpoint: String,
    pub method: Method,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "targetType")]
    pub target_type: Option<TargetType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// API response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub meta: ResponseMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<ApiError>>,
}

/// Response metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub method: Method,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub response_type: Option<String>,
}

/// API error structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub detail: String,
}

/// API methods
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Read,
    Write,
    Update,
    Subscribe,
    Create,
    Delete,
    Select,
    Notify,
}

/// Target type for requests
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetType {
    Room,
    Device,
}

impl Request {
    /// Create a new request with the given endpoint and method
    pub fn new(endpoint: impl Into<String>, method: Method) -> Self {
        Self {
            meta: RequestMeta {
                id: Uuid::new_v4(),
                endpoint: endpoint.into(),
                method,
                target_type: None,
                target: None,
            },
            data: None,
        }
    }

    /// Set the target type and ID
    pub fn with_target(mut self, target_type: TargetType, target: impl Into<String>) -> Self {
        self.meta.target_type = Some(target_type);
        self.meta.target = Some(target.into());
        self
    }

    /// Set the request data
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Get the request ID
    pub fn id(&self) -> Uuid {
        self.meta.id
    }
}

impl Response {
    /// Check if the response contains errors
    pub fn has_errors(&self) -> bool {
        self.errors.as_ref().is_some_and(|e| !e.is_empty())
    }

    /// Get the first error message, if any
    pub fn error_message(&self) -> Option<String> {
        self.errors
            .as_ref()
            .and_then(|e| e.first())
            .map(|e| e.detail.clone())
    }
}
