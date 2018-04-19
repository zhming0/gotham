//! Introduces the Middleware and Pipeline concepts provided by the Gotham web framework.

extern crate futures;
extern crate gotham;
#[macro_use]
extern crate gotham_derive;
extern crate hyper;
extern crate mime;

use futures::{future, Future};
use hyper::{Response, StatusCode};
use hyper::header::{Headers, UserAgent};
use gotham::handler::HandlerFuture;
use gotham::middleware::Middleware;
use gotham::protocol::response::create_response;
use gotham::router::Router;
use gotham::router::builder::*;
use gotham::state::{FromState, State};
use gotham::pipeline::new_pipeline;
use gotham::pipeline::single::single_pipeline;

/// A simple struct which holds an identifier for the user agent which made the request.
///
/// It is created by our Middleware and then accessed via `state` by both our Middleware and Handler.
#[derive(StateData)]
pub struct ExampleMiddlewareData {
    pub user_agent: String,
    pub supported: bool,
}

/// A struct that can act as a Gotham web framework middleware.
///
/// The key requirements for struct to act as a Middleware are:
///
///     1. That the struct implements the `gotham::middleware::NewMiddleware` trait which allows
///        the Gotham web framework to create a new instance of your middleware to service every
///        request. In many cases, as we're doing here, this can simply be derived.
///     2. That the struct implements the `gotham::middleware::Middleware` trait as we're doing
///        next.
#[derive(Clone, NewMiddleware)]
pub struct ExampleMiddleware;

/// Implementing `gotham::middleware::Middleware` allows the logic that you want your Middleware to
/// provided to be correctly executed by the Gotham web framework Router.
///
/// As shown here Middleware can make changes to the environment both before and after the handler^
/// for the route is executed.
///
/// ^Later examples will show how Middlewares in a pipeline can work with each other in a similar
/// manner.
impl Middleware for ExampleMiddleware {
    fn call<Chain>(self, mut state: State, chain: Chain) -> Box<HandlerFuture>
    where
        Chain: FnOnce(State) -> Box<HandlerFuture>,
    {
        let user_agent = {
            let headers = Headers::borrow_from(&state);
            match headers.get::<UserAgent>() {
                Some(ua) => ua.to_string(),
                None => String::from("None"),
            }
        };

        // Prior to letting Request handling proceed our middleware creates some new data and adds
        // it to `state`.
        state.put(ExampleMiddlewareData {
            user_agent,
            supported: false,
        });

        // We're finished working on the Request, so allow other components to continue processing
        // the Request.
        //
        // Alternatively we could elect to not call chain and return a Response we've created if we
        // want to prevent any further processing from occuring on the Request.
        let result = chain(state);

        // Once a Response is generated by another part of the application, in this example's case
        // the middleware_reliant_handler function, we want to do some more work.
        //
        // The syntax used here is part of the async environment in which the Gotham web framework
        // operates, you may not have encountered this before. For more details you can read about
        // the Tokio project at https://tokio.rs/docs/getting-started/hello-world/
        let f = result.and_then(move |(state, mut response)| {
            {
                let headers = response.headers_mut();
                let data = ExampleMiddlewareData::borrow_from(&state);

                // All our middleware does is add a header to the Response generated by our handler.
                headers.set_raw(
                    "X-User-Agent",
                    format!(
                        "Supplied: {}, Supported: {}",
                        data.user_agent, data.supported
                    ),
                );
            };
            future::ok((state, response))
        });

        Box::new(f)
    }
}

/// The handler which is invoked for all requests to "/".
///
/// This handler expects that `ExampleMiddleware` has already been executed by Gotham before
/// it is invoked. As a result of that middleware being run our handler trusts that it must
/// have placed data into state that we can perform operations on.
pub fn middleware_reliant_handler(mut state: State) -> (State, Response) {
    {
        let data = ExampleMiddlewareData::borrow_mut_from(&mut state);

        // Mark any kind of web client as supported. A trival example but it highlights the
        // interaction that is possible between Middleware and Handlers via state.
        data.supported = true;
    };

    // Finally we create a basic Response to complete our handling of the Request.
    let res = create_response(&state, StatusCode::Ok, None);
    (state, res)
}

/// Create a `Router`
fn router() -> Router {
    // Within the Gotham web framework Middleware is added to and referenced from a Pipeline.
    //
    // A pipeline can consist of multiple Middleware types and guarantees to call them all in the
    // ordering which is established by successive calls to the `add` method.
    //
    // A pipeline is considered complete once the build method is called and can no longer
    // be modified.
    //
    // The Gotham web framework supports multiple Pipelines and even Pipelines containing Pipelines.
    // However, as shown here, many applications will get sufficent power and flexibility
    // from a `single_pipeline` which we've provided specific API assitance for.
    let (chain, pipelines) = single_pipeline(new_pipeline().add(ExampleMiddleware).build());

    // Notice we've switched from build_simple_router which has been present in all our examples up
    // until this point. Under the hood build_simple_router has simply been creating an empty
    // set of Pipelines on your behalf.
    //
    // Now that we're creating and populating our own Pipelines we'll switch to using
    // build_router directly.
    //
    // Tip: Use build_simple_router for as long as you can. Switching to build_router is simple once
    // do need to introduce Pipelines and Middleware.
    build_router(chain, pipelines, |route| {
        route.get("/").to(middleware_reliant_handler);
    })
}

/// Start a server and use a `Router` to dispatch requests
pub fn main() {
    let addr = "127.0.0.1:7878";
    println!("Listening for requests at http://{}", addr);
    gotham::start(addr, router())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gotham::test::TestServer;

    #[test]
    fn ensure_middleware_and_handler_collaborate() {
        let test_server = TestServer::new(router()).unwrap();
        let response = test_server
            .client()
            .get("http://localhost")
            .with_header(UserAgent::new("TestServer/0.0.0"))
            .perform()
            .unwrap();

        assert_eq!(response.status(), StatusCode::Ok);

        // Ensure Middleware has set a header after our handler generated the Response.
        assert_eq!(
            response.headers().get_raw("X-User-Agent").unwrap(),
            "Supplied: TestServer/0.0.0, Supported: true"
        );
    }
}
