use std::{sync::Arc, time::Duration as StdDuration};

use super::service::AuthService;

pub const MAINTENANCE_INTERVAL: StdDuration = StdDuration::from_secs(12 * 60 * 60);

pub async fn run_maintenance_once(service: &AuthService) {
    service.run_maintenance_cycle().await;
}

pub async fn run_maintenance_loop(service: Arc<AuthService>) {
    let mut ticks = tokio::time::interval(MAINTENANCE_INTERVAL);
    // Consume the immediate tick before the startup scan, so the next wake-up
    // is exactly one maintenance interval after the loop starts.
    ticks.tick().await;
    run_maintenance_once(&service).await;
    loop {
        ticks.tick().await;
        run_maintenance_once(&service).await;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::json;

    use super::*;
    use crate::{
        auth_provider::service::Clock,
        auth_provider::{
            LoginResult, LoginRuntime, Provider, ProviderError, ProviderKind, ProviderLoginContext,
            ProviderModels, ProviderPayload, ProviderRegistry, ProviderRequest, RefreshedPayload,
        },
        db::{models::AuthAccount, models::AuthAccountUpsert, repository::Repository},
    };

    const NOW: &str = "2026-08-09T00:00:00Z";

    struct FixedClock(DateTime<Utc>);
    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    struct FakeProvider {
        refreshes: Mutex<HashMap<String, usize>>,
        models: Mutex<HashMap<String, usize>>,
        operations: AtomicUsize,
        kind: ProviderKind,
    }
    impl Default for FakeProvider {
        fn default() -> Self {
            Self {
                refreshes: Mutex::new(HashMap::new()),
                models: Mutex::new(HashMap::new()),
                operations: AtomicUsize::new(0),
                kind: ProviderKind::Codex,
            }
        }
    }

    impl FakeProvider {
        fn refreshes_for(&self, token: &str) -> usize {
            *self.refreshes.lock().unwrap().get(token).unwrap_or(&0)
        }

        fn models_for(&self, token: &str) -> usize {
            *self.models.lock().unwrap().get(token).unwrap_or(&0)
        }

        fn token(payload: &ProviderPayload) -> String {
            payload
                .as_value()
                .get("refresh_token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        }

        fn count(counts: &Mutex<HashMap<String, usize>>, token: String) {
            *counts.lock().unwrap().entry(token).or_default() += 1;
        }
    }

    #[async_trait]
    impl Provider for FakeProvider {
        fn kind(&self) -> ProviderKind {
            self.kind.clone()
        }

        async fn login(
            &self,
            _: &ProviderLoginContext,
            _: &dyn LoginRuntime,
        ) -> Result<LoginResult, ProviderError> {
            Err(ProviderError::LoginFailed)
        }

        async fn import(&self, _: &[u8]) -> Result<LoginResult, ProviderError> {
            Err(ProviderError::ImportFailed)
        }

        async fn refresh(
            &self,
            payload: &ProviderPayload,
        ) -> Result<RefreshedPayload, ProviderError> {
            let token = Self::token(payload);
            Self::count(&self.refreshes, token.clone());
            self.operations.fetch_add(1, Ordering::SeqCst);
            if token == "unauthorized" {
                return Err(ProviderError::Unauthorized);
            }
            if token == "payment-required" {
                return Err(ProviderError::PaymentRequired);
            }
            if token == "fails" {
                return Err(ProviderError::Retryable);
            }
            Ok(RefreshedPayload {
                payload: ProviderPayload::new(json!({
                    "access_token": "fresh-access",
                    "refresh_token": token,
                    // The maintenance tests pin the clock at NOW, so a refreshed
                    // token stays due for the next pass.  `sync_models` refreshes
                    // before listing, so a due account incurs two refreshes per
                    // pass (cycle refresh + refresh-before-sync) and one listing.
                    "expires_at": "2026-08-09T00:01:00Z"
                })),
                last_refreshed_at: Some(NOW.into()),
                next_refresh_after: None,
                next_retry_after: None,
            })
        }

        async fn outbound(
            &self,
            _: ProviderRequest<'_>,
        ) -> Result<reqwest::Response, ProviderError> {
            Err(ProviderError::Retryable)
        }

        async fn list_models(
            &self,
            _account: &AuthAccount,
            payload: &ProviderPayload,
        ) -> Result<ProviderModels, ProviderError> {
            let token = Self::token(payload);
            Self::count(&self.models, token.clone());
            if token == "models-fail" {
                return Err(ProviderError::Retryable);
            }
            if token == "models-payment" {
                return Err(ProviderError::PaymentRequired);
            }
            Ok(vec![])
        }
    }

    async fn repository() -> Arc<Repository> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        Arc::new(Repository::new(pool))
    }

    async fn add_account_for(
        repository: &Repository,
        provider: &str,
        name: &str,
        expires_at: &str,
        status: &str,
        retry: Option<&str>,
    ) -> String {
        let account = repository
            .upsert_by_provider_account_id(&AuthAccountUpsert {
                provider: provider.into(),
                label: name.into(),
                account_id: name.into(),
                attributes: json!({}),
                payload: json!({"access_token": "access", "refresh_token": name, "expires_at": expires_at}),
                last_refreshed_at: None,
                next_refresh_after: None,
                next_retry_after: retry.map(str::to_owned),
            })
            .await
            .unwrap();
        if status == "invalid" {
            repository.mark_invalid(&account.id, retry, None).await.unwrap();
        }
        account.id
    }

    fn service(repository: Arc<Repository>, provider: Arc<FakeProvider>) -> Arc<AuthService> {
        let mut registry = ProviderRegistry::new();
        registry.register(provider);
        Arc::new(AuthService::with_clock(
            repository,
            registry,
            Arc::new(FixedClock(NOW.parse().unwrap())),
        ))
    }

    async fn yield_until(label: &str, mut condition: impl FnMut() -> bool) {
        // The loop drives SQLite work in a spawned task.  Under the full test
        // suite that task can need more than 100 cooperative yields to obtain
        // its connection; keep this test-only wait deterministic without
        // adding wall-clock sleeps to a paused-time scheduler test.
        for _ in 0..10_000 {
            if condition() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("maintenance loop did not reach expected state: {label}");
    }

    #[tokio::test]
    async fn startup_scan_and_each_twelve_hour_tick_only_process_due_accounts() {
        let repository = repository().await;
        add_account_for(
            &repository,
            "codex",
            "due",
            "2026-08-09T00:01:00Z",
            "active",
            None,
        )
        .await;
        add_account_for(
            &repository,
            "codex",
            "not-due",
            "2026-08-09T02:00:00Z",
            "active",
            None,
        )
        .await;
        let provider = Arc::new(FakeProvider::default());
        let service = service(repository, provider.clone());

        let loop_handle = tokio::spawn(run_maintenance_loop(service));
        // Both accounts are active.  The pinned clock keeps "due" due on every
        // pass, so per pass "due" does one lazy refresh inside `sync_models`
        // (the cycle no longer refreshes separately) plus one model listing.
        // "not-due" has a fresh token: it never refreshes, but the 12h cycle
        // still refreshes its model snapshot (design §7 step 3 / ADR-8) — one
        // listing per pass.
        yield_until("startup", || {
            provider.refreshes_for("due") == 1
                && provider.models_for("due") == 1
                && provider.models_for("not-due") == 1
        })
        .await;
        assert_eq!(provider.refreshes_for("not-due"), 0);
        assert_eq!(provider.models_for("not-due"), 1);

        // Advance the scheduler under paused Tokio time; resume before the
        // SQL-backed scan runs because sqlx's pool acquisition uses Tokio time.
        tokio::task::yield_now().await;
        tokio::time::pause();
        tokio::time::advance(MAINTENANCE_INTERVAL).await;
        tokio::time::resume();
        yield_until("first scheduled tick", || {
            provider.refreshes_for("due") == 2
                && provider.models_for("due") == 2
                && provider.models_for("not-due") == 2
        })
        .await;
        assert_eq!(provider.refreshes_for("not-due"), 0);
        assert_eq!(provider.models_for("not-due"), 2);
        loop_handle.abort();
    }

    #[tokio::test]
    async fn invalid_recovery_and_failure_are_isolated_per_account() {
        let repository = repository().await;
        let recovered = add_account_for(
            &repository,
            "codex",
            "recover",
            "2026-08-09T03:00:00Z",
            "invalid",
            None,
        )
        .await;
        let failed = add_account_for(
            &repository,
            "codex",
            "fails",
            "2026-08-09T03:00:00Z",
            "invalid",
            None,
        )
        .await;
        add_account_for(
            &repository,
            "codex",
            "models-fail",
            "2026-08-09T00:01:00Z",
            "active",
            None,
        )
        .await;
        add_account_for(
            &repository,
            "codex",
            "active",
            "2026-08-09T00:01:00Z",
            "active",
            None,
        )
        .await;
        let provider = Arc::new(FakeProvider::default());
        let service = service(repository.clone(), provider.clone());

        run_maintenance_once(&service).await;

        assert_eq!(
            repository
                .get_auth_account(&recovered)
                .await
                .unwrap()
                .status,
            "active"
        );
        let failed = repository.get_auth_account(&failed).await.unwrap();
        assert_eq!(failed.status, "invalid");
        assert_eq!(
            failed.next_retry_after.as_deref(),
            Some("2026-08-09T12:00:00+00:00")
        );
        // An active account is driven through `sync_models`, whose internal
        // lazy refresh runs exactly once for a due token (design §7 step 3).
        assert_eq!(provider.refreshes_for("active"), 1);
        assert_eq!(provider.models_for("active"), 1);
        assert_eq!(provider.models_for("models-fail"), 1);
    }

    // --- C9: Kimi accounts join the same provider-neutral maintenance cycle ---

    #[tokio::test]
    async fn kimi_active_account_joins_model_sync_maintenance() {
        let repository = repository().await;
        let id = add_account_for(
            &repository,
            "kimi",
            "kimi-due",
            "2026-08-09T00:01:00Z",
            "active",
            None,
        )
        .await;
        let provider = Arc::new(FakeProvider {
            kind: ProviderKind::Kimi,
            ..FakeProvider::default()
        });
        let service = service(repository.clone(), provider.clone());

        // One maintenance pass: the due Kimi account refreshes then lists models.
        crate::auth_provider::maintenance::run_maintenance_once(&service).await;

        assert_eq!(provider.refreshes_for("kimi-due"), 1);
        assert_eq!(provider.models_for("kimi-due"), 1);
        let stored = repository.get_auth_account(&id).await.unwrap();
        assert_eq!(stored.provider, "kimi");
        assert_eq!(stored.status, "active");
    }

    #[tokio::test]
    async fn kimi_invalid_account_with_due_refresh_recovers() {
        let repository = repository().await;
        let id = add_account_for(
            &repository,
            "kimi",
            "kimi-recover",
            "2026-08-09T03:00:00Z",
            "invalid",
            Some("2026-08-08T00:00:00Z"),
        )
        .await;
        let provider = Arc::new(FakeProvider {
            kind: ProviderKind::Kimi,
            ..FakeProvider::default()
        });
        let service = service(repository.clone(), provider.clone());

        crate::auth_provider::maintenance::run_maintenance_once(&service).await;

        let stored = repository.get_auth_account(&id).await.unwrap();
        assert_eq!(stored.status, "active");
        // force refresh (recovery) + lazy refresh inside the follow-up model
        // sync (token still due under the pinned clock).
        assert_eq!(provider.refreshes_for("kimi-recover"), 2);
    }

    #[tokio::test]
    async fn kimi_refresh_unauthorized_keeps_account_invalid() {
        let repository = repository().await;
        // "unauthorized" is the refresh token the fake maps to a Kimi
        // `ProviderError::Unauthorized` (real Kimi 401/403/invalid_grant →
        // Unauthorized classification).  The maintenance loop must keep the
        // account invalid and schedule a retry.
        let id = add_account_for(
            &repository,
            "kimi",
            "unauthorized",
            "2026-08-09T03:00:00Z",
            "invalid",
            Some("2026-08-08T00:00:00Z"),
        )
        .await;
        let provider = Arc::new(FakeProvider {
            kind: ProviderKind::Kimi,
            ..FakeProvider::default()
        });
        let service = service(repository.clone(), provider.clone());

        crate::auth_provider::maintenance::run_maintenance_once(&service).await;

        let stored = repository.get_auth_account(&id).await.unwrap();
        assert_eq!(stored.status, "invalid");
        assert_eq!(
            stored.next_retry_after.as_deref(),
            Some("2026-08-09T12:00:00+00:00")
        );
    }

    #[tokio::test]
    async fn active_account_with_dead_subscription_is_marked_invalid_by_maintenance() {
        let repository = repository().await;
        // The account name is both label and refresh token; the fake maps the
        // "models-payment" token to a PaymentRequired list_models failure.
        let id = add_account_for(
            &repository,
            "kimi",
            "models-payment",
            "2026-08-09T00:01:00Z",
            "active",
            None,
        )
        .await;
        let provider = Arc::new(FakeProvider {
            kind: ProviderKind::Kimi,
            ..FakeProvider::default()
        });
        let service = service(repository.clone(), provider.clone());

        crate::auth_provider::maintenance::run_maintenance_once(&service).await;

        // A dead subscription is terminal: the account leaves routing and gets
        // a backoff, rather than staying active and retrying 12h forever.
        let stored = repository.get_auth_account(&id).await.unwrap();
        assert_eq!(stored.status, "invalid");
        assert!(stored.next_retry_after.is_some());
    }
}
