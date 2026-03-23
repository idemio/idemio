pub struct RouteKey<'a> {
    pub path: Option<&'a str>,
    pub method: Option<&'a str>,
}

impl<'a> RouteKey<'a> {
    pub fn new(path: &'a str, method: &'a str) -> Self {
        Self { path: Some(path), method: Some(method) }
    }
}

pub trait RouteKeyParser<I>
where
    I: Send + Sync,
{

    fn as_route_key<'a>(
        &self,
        request: &'a I,
    ) -> RouteKey<'a>;
}