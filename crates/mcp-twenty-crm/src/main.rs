use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    mcp_twenty_crm::server().main().await
}
