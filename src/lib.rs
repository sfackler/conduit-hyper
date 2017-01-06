extern crate conduit;
extern crate hyper;
extern crate semver;

use hyper::server::Request as HyperRequest;
use hyper::version::HttpVersion;
use hyper::method::Method;
use hyper::header::{Host, ContentLength};
use hyper::uri::RequestUri;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::io::Read;
use std::str;

pub struct Request<'a, 'b: 'a> {
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

pub struct Headers(HashMap<String, Vec<String>>);

impl conduit::Headers for Headers {
    fn find(&self, key: &str) -> Option<Vec<&str>> {
        self.0.get(key).map(|vs| vs.iter().map(|v| &**v).collect())
    }

    fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    fn all(&self) -> Vec<(&str, Vec<&str>)> {
        self.0.iter().map(|(k, vs)| (&**k, vs.iter().map(|v| &**v).collect())).collect()
    }
}
