use std::sync::Arc;

mod server;
mod handlers;
mod database;
mod indexer;
mod workspace;
mod cache;
mod config;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("neodos_lsp=info,trace")
    )
    .format_timestamp_millis()
    .init();

    log::info!("NeoDOS LSP v{} starting", env!("CARGO_PKG_VERSION"));
    log::info!("target: {} / {}", std::env::consts::ARCH, std::env::consts::OS);

    let config = Arc::new(config::NeodosLspConfig::from_env());
    log::info!(
        "config: workspace_max_files={}, cache_size={}, indexing={:?}",
        config.workspace.max_files,
        config.cache.documents,
        config.indexing.strategy,
    );

    let mut srv = server::LspServer::new(config);
    if let Err(e) = srv.run() {
        log::error!("server exited with error: {e}");
        std::process::exit(1);
    }

    log::info!("NeoDOS LSP shutdown complete");
}
