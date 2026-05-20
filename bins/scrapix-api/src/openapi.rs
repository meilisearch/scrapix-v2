//! OpenAPI specification for the Scrapix API.
//!
//! Generates an OpenAPI 3.1 spec from annotated handlers and types,
//! and serves it at `/openapi.json` with a Scalar UI at `/docs`.

use utoipa::OpenApi;

/// Scrapix API — OpenAPI specification
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Scrapix API",
        version = "0.1.0",
        description = "High-performance web crawler and search indexer API. Scrape pages, map websites, run distributed crawls, and search indexed content.",
        contact(name = "Meilisearch", url = "https://scrapix.meilisearch.com"),
        license(name = "MIT")
    ),
    servers(
        (url = "https://scrapix.meilisearch.dev", description = "Production"),
        (url = "http://localhost:8080", description = "Local development")
    ),
    tags(
        (name = "health", description = "Health and diagnostics"),
        (name = "scrape", description = "Single-page scraping"),
        (name = "map", description = "Website URL discovery"),
        (name = "search", description = "Search indexed content"),
        (name = "crawl", description = "Distributed crawl jobs"),
        (name = "jobs", description = "Job management"),
        (name = "configs", description = "Saved crawl configurations"),
        (name = "engines", description = "Meilisearch engine registry"),
        (name = "auth", description = "Authentication and account management")
    ),
    paths(
        // Health & diagnostics
        crate::health,
        crate::health_services,
        crate::handle_stats,
        crate::handle_errors,
        crate::handle_domains,
        // Core endpoints
        crate::scrape_url,
        crate::map_url,
        crate::search_url,
        crate::create_crawl,
        crate::create_crawl_sync,
        crate::create_crawl_bulk,
        // Job management
        crate::list_jobs,
        crate::job_status,
        crate::cancel_job,
        // Configs
        crate::configs::create_config,
        crate::configs::list_configs,
        crate::configs::get_config,
        crate::configs::update_config,
        crate::configs::delete_config,
        crate::configs::trigger_config,
        // Engines
        crate::engines::create_engine,
        crate::engines::list_engines,
        crate::engines::get_engine,
        crate::engines::update_engine,
        crate::engines::delete_engine,
        crate::engines::set_default_engine,
        crate::engines::list_engine_indexes,
        crate::engines::search_engine_index,
        // Auth
        crate::auth::handlers::signup,
        crate::auth::handlers::login,
        crate::auth::handlers::logout,
        crate::auth::handlers::get_me,
        crate::auth::handlers::update_me,
        crate::auth::handlers::get_account,
        crate::auth::handlers::update_account,
        crate::auth::handlers::list_api_keys,
        crate::auth::handlers::create_api_key,
        crate::auth::handlers::revoke_api_key,
        crate::auth::handlers::get_billing,
        crate::auth::handlers::update_billing,
        crate::auth::handlers::topup_credits,
        crate::auth::handlers::update_auto_topup,
        crate::auth::handlers::update_spend_limit,
        crate::auth::handlers::list_transactions,
    ),
    components(schemas(
        // Core API types
        crate::HealthResponse,
        crate::ServiceHealthResponse,
        crate::ServiceStatus,
        crate::ScrapeRequest,
        crate::ScrapeResponse,
        crate::ScrapeFormat,
        crate::ScrapeMetadata,
        crate::AiOptions,
        crate::AiExtractOptions,
        crate::AiFieldDef,
        crate::AiResult,
        crate::MapRequest,
        crate::MapResponse,
        crate::MapLink,
        crate::SearchRequest,
        crate::CreateCrawlResponse,
        crate::BulkCrawlResponse,
        crate::BulkCrawlError,
        crate::JobStatusResponse,
        crate::ApiError,
        // Diagnostic types
        crate::SystemStatsResponse,
        crate::MeilisearchStats,
        crate::JobSummary,
        crate::DiagnosticsStats,
        crate::ErrorsResponse,
        crate::ErrorRecord,
        crate::DomainsResponse,
        crate::DomainInfo,
        // Config types
        crate::configs::CrawlConfigRecord,
        crate::configs::CreateConfigRequest,
        crate::configs::UpdateConfigRequest,
        crate::configs::TriggerResponse,
        // Engine types
        crate::engines::EngineRecord,
        crate::engines::CreateEngineRequest,
        crate::engines::UpdateEngineRequest,
        crate::engines::EngineIndex,
        // Auth types
        crate::auth::handlers::SignupRequest,
        crate::auth::handlers::LoginRequest,
        crate::auth::handlers::UpdateMeRequest,
        crate::auth::handlers::UpdateAccountRequest,
        crate::auth::handlers::CreateApiKeyRequest,
        crate::auth::handlers::UpdateBillingRequest,
        crate::auth::handlers::TopupRequest,
        crate::auth::handlers::AutoTopupRequest,
        crate::auth::handlers::SpendLimitRequest,
    )),
    security(
        ("api_key" = [])
    ),
    modifiers(&SecurityAddon)
)]
pub struct ScrapixApi;

/// Adds the API key security scheme to the OpenAPI spec.
struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "api_key",
                utoipa::openapi::security::SecurityScheme::ApiKey(
                    utoipa::openapi::security::ApiKey::Header(
                        utoipa::openapi::security::ApiKeyValue::new("x-api-key"),
                    ),
                ),
            );
        }
    }
}
