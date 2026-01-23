//! MCP (Model Context Protocol) server implementation
//!
//! JSON-RPC over stdio for AI tool integration.

pub mod protocol;
pub mod tools;

pub use protocol::{
    methods, InitializeResult, McpHandler, McpRequest, McpResponse, McpServer, ToolCallResult,
};
pub use tools::{get_tool_definitions, TOOL_DEFINITIONS};
