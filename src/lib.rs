extern crate conduit;
extern crate hyper;
extern crate semver;
extern crate unicase;

#[macro_use]
extern crate log;

use hyper::server::{Request as HyperRequest, Listening, Response, Fresh};
use hyper::version::HttpVersion;
use hyper::method::Method;
use hyper::header::{Host, ContentLength};
use hyper::uri::RequestUri;
use hyper::net::{HttpListener, HttpsListener, SslServer, NetworkListener};
use hyper::status::StatusCode;
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::io::{self, Read, Cursor};
use std::str;
use unicase::UniCase;

struct Request<'a, 'b: 'a> {
    request: HyperRequest<'a, 'b>,
    scheme: conduit::Scheme,
    headers: Headers,
    extensions: conduit::Extensions,
}

fn ver(major: u64, minor: u64) -> semver::Version {
    semver::Version {
        major: major,
        minor: minor,
        patch: 0,
        pre: vec!(),
        build: vec!()
    }
}

impl<'a, 'b> conduit::Request for Request<'a, 'b> {
    fn http_version(&self) -> semver::Version {
        match self.request.version {
            HttpVersion::Http09 => ver(0, 9),
            HttpVersion::Http10 => ver(1, 0),
            HttpVersion::Http11 => ver(1, 1),
            HttpVersion::Http20 => ver(2, 0),
        }
    }

    fn conduit_version(&self) -> semver::Version {
        ver(0, 1)
    }

    fn method(&self) -> conduit::Method {
        match self.request.method {
            Method::Connect => conduit::Method::Connect,
            Method::Delete => conduit::Method::Delete,
            Method::Get => conduit::Method::Get,
            Method::Head => conduit::Method::Head,
            Method::Options => conduit::Method::Options,
            Method::Patch => conduit::Method::Patch,
            Method::Post => conduit::Method::Post,
            Method::Put => conduit::Method::Put,
            Method::Trace => conduit::Method::Trace,
            // https://github.com/conduit-rust/conduit/pull/12
            Method::Extension(_) => unimplemented!(),
        }
    }

    fn scheme(&self) -> conduit::Scheme {
        self.scheme
    }

    fn host<'c>(&'c self) -> conduit::Host<'c> {
        conduit::Host::Name(&self.request.headers.get::<Host>().unwrap().hostname)
    }

    fn virtual_root(&self) -> Option<&str> {
        None
    }

    fn path(&self) -> &str {
        match self.request.uri {
            RequestUri::AbsolutePath(ref s) => s.split('?').next().unwrap(),
            _ => panic!("unsupported request type"),
        }
    }

    fn query_string(&self) -> Option<&str> {
        match self.request.uri {
            RequestUri::AbsolutePath(ref s) => s.splitn(2, '?').nth(1),
            _ => panic!("unsupported request type"),
        }
    }

    fn remote_addr(&self) -> SocketAddr {
        self.request.remote_addr
    }

    fn content_length(&self) -> Option<u64> {
        self.request.headers.get::<ContentLength>().map(|h| h.0)
    }

    fn headers(&self) -> &conduit::Headers {
        &self.headers
    }

    fn body(&mut self) -> &mut Read {
        &mut self.request
    }

    fn extensions(&self) -> &conduit::Extensions {
        &self.extensions
    }

    fn mut_extensions(&mut self) -> &mut conduit::Extensions {
        &mut self.extensions
    }
}

struct Headers(Vec<(String, Vec<String>)>);

impl Headers {
    fn find_raw(&self, key: &str) -> Option<&[String]> {
        self.0.iter().find(|&&(ref k, _)| UniCase(k) == key).map(|&(_, ref vs)| &**vs)
    }
}

impl conduit::Headers for Headers {
    fn find(&self, key: &str) -> Option<Vec<&str>> {
        self.find_raw(key).map(|vs| vs.iter().map(|v| &**v).collect())
    }

    fn has(&self, key: &str) -> bool {
        self.find_raw(key).is_some()
    }

    fn all(&self) -> Vec<(&str, Vec<&str>)> {
        self.0.iter().map(|&(ref k, ref vs)| (&**k, vs.iter().map(|v| &**v).collect())).collect()
    }
}

pub struct Server<L> {
    server: hyper::Server<L>,
    scheme: conduit::Scheme,
}

impl Server<HttpListener> {
    pub fn http<T>(addr: T) -> hyper::Result<Server<HttpListener>>
        where T: ToSocketAddrs
    {
        Ok(Server {
            server: hyper::Server::http(addr)?,
            scheme: conduit::Scheme::Http,
        })
    }
}

impl<S> Server<HttpsListener<S>>
    where S: SslServer + Clone + Send
{
    pub fn https<T>(addr: T, ssl: S) -> hyper::Result<Server<HttpsListener<S>>>
        where T: ToSocketAddrs,
    {
        Ok(Server {
            server: hyper::Server::https(addr, ssl)?,
            scheme: conduit::Scheme::Https,
        })
    }
}

impl<L> Server<L>
    where L: NetworkListener + Send + 'static
{
    pub fn new(listener: L, scheme: conduit::Scheme) -> Server<L> {
        Server {
            server: hyper::Server::new(listener),
            scheme: scheme,
        }
    }

    /// Returns a mutable reference to the inner Hyper `Server`.
    pub fn as_mut(&mut self) -> &mut hyper::Server<L> {
        &mut self.server
    }

    pub fn handle<H>(self, handler: H) -> hyper::Result<Listening>
        where H: conduit::Handler
    {
        let handler = Handler {
            handler: handler,
            scheme: self.scheme,
        };
        self.server.handle(handler)
    }

    pub fn handle_threads<H>(self, handler: H, threads: usize) -> hyper::Result<Listening>
        where H: conduit::Handler
    {
        let handler = Handler {
            handler: handler,
            scheme: self.scheme,
        };
        self.server.handle_threads(handler, threads)
    }
}

struct Handler<H> {
    handler: H,
    scheme: conduit::Scheme,
}

impl<H> hyper::server::Handler for Handler<H>
    where H: conduit::Handler
{
    fn handle<'a, 'k>(&'a self, request: HyperRequest<'a, 'k>, mut response: Response<'a, Fresh>) {
        let mut headers = HashMap::new();
        for header in request.headers.iter() {
            headers.entry(header.name().to_owned())
                .or_insert_with(Vec::new)
                .push(header.value_string());
        }
        let mut request = Request {
            request: request,
            scheme: self.scheme,
            headers: Headers(headers.into_iter().collect()),
            extensions: conduit::Extensions::new(),
        };

        let mut resp = match self.handler.call(&mut request) {
            Ok(response) => response,
            Err(e) => {
                error!("Unhandled error: {}", e);
                conduit::Response {
                    status: (500, "Internal Server Error"),
                    headers: HashMap::new(),
                    body: Box::new(Cursor::new(e.to_string().into_bytes())),
                }
            }
        };

        *response.status_mut() = StatusCode::from_u16(resp.status.0 as u16);
        for (key, value) in resp.headers {
            let value = value.into_iter().map(|s| s.into_bytes()).collect();
            response.headers_mut().set_raw(key, value);
        }

        if let Err(e) = respond(response, &mut resp.body) {
            error!("Error sending response: {}", e);
        }
    }
}

fn respond<'a>(response: Response<'a, Fresh>, body: &mut Box<conduit::WriteBody + Send>) -> io::Result<()> {
    let mut response = response.start()?;
    body.write_body(&mut response)?;
    response.end()
}
