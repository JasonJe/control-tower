//! Global API authentication middleware.
//!
//! Protects every `/api/*` route when auth is enabled, instead of relying on
//! each handler to call `require_auth` (which is easy to forget for new
//! endpoints). Login/setup endpoints and static assets stay reachable.
//!
//! `check_auth_header` is used rather than a raw token check so that the
//! middleware follows exactly the same rules as the handlers: auth is only
//! enforced when it is enabled *and* a password has been configured.

use std::future::{ready, Future, Ready};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use actix_web::body::EitherBody;
use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::{web, HttpResponse};

use crate::api::{check_auth_header, ApiResponse};
use crate::ServiceState;

/// Routes that must remain reachable without a session token.
fn is_public(path: &str) -> bool {
    // Static assets (`/`, `/vendor/*`) and anything outside the API.
    if !path.starts_with("/api/") {
        return true;
    }

    // Auth endpoints: the client needs these before it has a token.
    // `/api/auth/setup-password` refuses to overwrite an existing password
    // (see `auth_handlers::setup_password`), so exposing it is safe.
    matches!(
        path,
        "/api/auth/nonce"
            | "/api/auth/status"
            | "/api/auth/login"
            | "/api/auth/setup-password"
            | "/api/auth/logout"
    )
}

/// Actix middleware that enforces authentication on protected API routes.
pub struct AuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Transform = AuthMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub struct AuthMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let public = is_public(req.path());

        // `check_auth_header` returns `true` when auth is not required, so this
        // is a no-op unless auth is enabled and a password has been configured.
        let auth_header = req
            .head()
            .headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok());
        let allowed = public
            || req
                .app_data::<web::Data<Arc<ServiceState>>>()
                .map(|state| check_auth_header(state.get_ref(), auth_header))
                .unwrap_or(false);

        if allowed {
            let service = Rc::clone(&self.service);
            Box::pin(async move {
                service
                    .call(req)
                    .await
                    .map(|res| res.map_into_left_body())
            })
        } else {
            let (req, _) = req.into_parts();
            let res = HttpResponse::Unauthorized()
                .json(ApiResponse::<()>::error("Unauthorized"));
            Box::pin(async move { Ok(ServiceResponse::new(req, res).map_into_right_body()) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_public;

    #[test]
    fn auth_endpoints_are_public() {
        for p in [
            "/api/auth/nonce",
            "/api/auth/status",
            "/api/auth/login",
            "/api/auth/setup-password",
            "/api/auth/logout",
        ] {
            assert!(is_public(p), "{p} should be public");
        }
    }

    #[test]
    fn assets_are_public() {
        assert!(is_public("/"));
        assert!(is_public("/vendor/crypto-js.min.js"));
    }

    #[test]
    fn data_endpoints_are_protected() {
        for p in [
            "/api/status",
            "/api/settings",
            "/api/profiles",
            "/api/profiles/x/refresh",
            "/api/connections",
            "/api/connections/history",
            "/api/config",
            "/api/logs",
            "/api/proxies",
            "/api/proxies/delay-all",
            "/api/rules",
            "/api/rule-providers/refresh-all",
        ] {
            assert!(!is_public(p), "{p} should be protected");
        }
    }
}
