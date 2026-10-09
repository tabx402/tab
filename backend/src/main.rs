mod accounts;
mod api;
mod auth;
mod bnb;
mod config;
mod credit;
mod db;
mod error;
mod finance;
mod financial;
mod intents;
mod invoices;
mod jobs;
mod models;
mod operator_demo;
mod products;
mod provider_search;
mod registry_cache;
mod runtime;
mod schema;
mod sponsor;
mod starter;
mod x402;
mod x402_service;
use crate::{auth::Auth, bnb::Bnb, config::Config, db::Store, error::Result};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Store,
    pub client: reqwest::Client,
    pub auth: Auth,
    pub bnb: Bnb,
    pub registry_cache: Arc<tokio::sync::RwLock<Option<registry_cache::RegistrySnapshot>>>,
}
impl AppState {
    pub fn new(config: Config) -> Result<Self> {
        let config = Arc::new(config);
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|_| crate::error::ApiError::internal())?;
        let store = Store::open(&config.database)?;
        let auth = Auth::new(config.app_id.clone(), client.clone());
        let bnb = Bnb::new(config.clone(), client.clone());
        Ok(Self {
            config,
            store,
            client,
            auth,
            bnb,
            registry_cache: Arc::new(tokio::sync::RwLock::new(None)),
        })
    }
}
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tab_api=info".into()),
        )
        .init();
    let config = Config::environment().map_err(|e| e.1)?;
    if std::env::args().any(|arg| arg == "--print-openapi") {
        println!("{}", serde_json::to_string_pretty(&schema::document())?);
        return Ok(());
    }
    let state = AppState::new(config).map_err(|e| e.1)?;
    if std::env::args().any(|arg| arg == "--init-database") {
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--operator-demo") {
        return operator_demo::serve(state).await.map_err(|e| e.1.into());
    }
    let address = state.config.bind.clone();
    let scheduled = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = scheduled.scheduled().await {
                tracing::warn!(status = error.0.as_u16(), "Scheduled run unavailable");
            }
        }
    });
    let relayer = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            relayer.reconcile_sponsorships().await;
        }
    });
    let registry = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if registry.refresh_registry().await.is_err() {
                tracing::warn!("Confirmed registry refresh unavailable");
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(address=%address,network=%state.config.network,"Tab Rust API listening");
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod chain_tests;

#[cfg(test)]
mod sponsor_smoke;
