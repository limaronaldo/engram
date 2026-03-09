//! MCP (Model Context Protocol) server implementation
//!
//! JSON-RPC over stdio for AI tool integration.

pub mod handlers;
pub mod protocol;
pub mod tools;

pub use protocol::{
    methods, InitializeResult, McpHandler, McpRequest, McpResponse, McpServer,
    MCP_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION_LEGACY,
    PromptArgument, PromptCapabilities, PromptContent, PromptDefinition, PromptMessage,
    ResourceCapabilities, ResourceDefinition, ResourceTemplate,
    ServerCapabilities, ToolAnnotations, ToolCallResult, ToolsCapability,
};
pub use tools::{get_tool_definitions, TOOL_DEFINITIONS};
