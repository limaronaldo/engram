//! MCP (Model Context Protocol) server implementation
//!
//! JSON-RPC over stdio for AI tool integration.

pub mod handlers;
pub mod http_transport;
#[cfg(feature = "grpc")]
pub mod grpc_transport;
pub mod prompts;
pub mod protocol;
pub mod resources;
pub mod tools;

pub use prompts::{get_prompt, list_prompts};
pub use protocol::{
    methods, InitializeResult, McpHandler, McpRequest, McpResponse, McpServer, PromptArgument,
    PromptCapabilities, PromptContent, PromptDefinition, PromptMessage, ResourceCapabilities,
    ResourceDefinition, ResourceTemplate, ServerCapabilities, ToolAnnotations, ToolCallResult,
    ToolsCapability, MCP_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION_LEGACY,
};
pub use resources::{list_resources, read_resource};
pub use tools::{get_tool_definitions, get_tool_definitions_tiered, TOOL_DEFINITIONS};
