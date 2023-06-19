#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![doc = include_str!("../../README.md")]

use async_graphql::{http::GraphiQLSource, ObjectType, Schema, SubscriptionType};
pub use async_graphql_axum::GraphQLSubscription;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    body::{Bytes, HttpBody},
    handler::Handler,
    headers::{authorization::Bearer, Authorization},
    http::{Request, StatusCode},
    response::{Html, IntoResponse, Response},
    BoxError, RequestExt, TypedHeader,
};
use derive_more::Constructor;
use opa_client::{AuthorizationToken, OPAClient};
use std::{future::Future, pin::Pin};

/// An [`axum`] [`Handler`] which provides the GraphiQL user interface, pre-configured for use with given GraphQL and websocket subscription endpoints.
///
/// # Example
/// ```
/// use axum::{routing::get, Router};
/// use graphql_endpoints::GraphiQLHandler;
///
/// fn add_graphiql_route(router: Router) -> Router {
///     router.route(
///         "/",
///         get(GraphiQLHandler::new(
///             "/graphql",
///             "/graphql/ws"
///         ))
///     )
/// }
/// ```
#[derive(Debug, Clone)]
pub struct GraphiQLHandler(Html<String>);

impl GraphiQLHandler {
    /// Constructs an instance of the handler with a given GraphQL endpoint and websocket subscription endpoint.
    pub fn new(graphql_endpoint: impl AsRef<str>, subscription_endpoint: impl AsRef<str>) -> Self {
        Self(Html(
            GraphiQLSource::build()
                .endpoint(graphql_endpoint.as_ref())
                .subscription_endpoint(subscription_endpoint.as_ref())
                .finish(),
        ))
    }
}

impl<S, B> Handler<((),), S, B> for GraphiQLHandler {
    type Future = Pin<Box<dyn Future<Output = Response> + Send + 'static>>;

    fn call(self, _req: Request<B>, _state: S) -> Self::Future {
        Box::pin(async { self.0.into_response() })
    }
}

/// An [`axum`] [`Handler`] which provides a GraphQL endpoint.
///
/// This endpoint extracts the [`AuthorizationToken`] and injects it into the GraphQL execution alongside an instance of the [`OPAClient`].
///
/// # Example
/// ```
/// use async_graphql::{ObjectType, Schema, SubscriptionType};
/// use axum::{routing::post, Router};
/// use graphql_endpoints::GraphQLHandler;
/// use opa_client::OPAClient;
/// use url::Url;
///
/// fn add_graphql_route<Query, Mutation, Subscription>(
///     router: Router
/// ) -> Router
/// where
///     Query: ObjectType + Clone + Default + 'static,
///     Mutation: ObjectType + Clone + Default + 'static,
///     Subscription: SubscriptionType + Clone + Default + 'static,
///  {
///     let schema = Schema::<Query, Mutation, Subscription>::default();
///     let opa_client = OPAClient::new(Url::parse("https://example.com:8181").unwrap());
///     router.route(
///         "/graphql",
///         post(GraphQLHandler::new(
///             schema,
///             opa_client
///         ))
///     )
/// }
/// ```
#[derive(Clone, Constructor)]
pub struct GraphQLHandler<Query, Mutation, Subscription> {
    schema: Schema<Query, Mutation, Subscription>,
    opa_client: OPAClient,
}

impl<S, B, Query, Mutation, Subscription> Handler<((),), S, B>
    for GraphQLHandler<Query, Mutation, Subscription>
where
    B: HttpBody + Unpin + Send + Sync + 'static,
    B::Data: Into<Bytes>,
    B::Error: Into<BoxError>,
    S: Send + Sync + 'static,
    Query: ObjectType + Clone + 'static,
    Mutation: ObjectType + Clone + 'static,
    Subscription: SubscriptionType + Clone + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send + 'static>>;

    fn call(self, mut req: Request<B>, _state: S) -> Self::Future {
        Box::pin(async move {
            let token = req
                .extract_parts::<TypedHeader<Authorization<Bearer>>>()
                .await
                .ok();
            let request = req.extract::<GraphQLRequest, _>().await;
            match request {
                Ok(request) => {
                    let token =
                        AuthorizationToken::new(token.map(|token| token.token().to_string()));
                    GraphQLResponse::from(
                        self.schema
                            .execute(request.into_inner().data(token).data(self.opa_client))
                            .await,
                    )
                    .into_response()
                }
                Err(err) => (StatusCode::BAD_REQUEST, err.0.to_string()).into_response(),
            }
        })
    }
}
