#[derive(Default)]
pub struct RouteKey<'a> {
    pub path: Option<&'a str>,
    pub method: Option<&'a str>,
}

impl<'a> RouteKey<'a> {
    pub fn new(path: &'a str, method: &'a str) -> Self {
        Self { path: Some(path), method: Some(method) }
    }

    #[inline]
    pub fn with_method(mut self, method: &'a str) -> Self {
        self.method = Some(method);
        self
    }

    #[inline]
    pub fn with_path(mut self, path: &'a str) -> Self {
        self.path = Some(path);
        self
    }
}

impl<'a> From<(&'a str, &'a str)> for RouteKey<'a> {
    fn from(value: (&'a str, &'a str)) -> Self {
        RouteKey::new(value.0, value.1)
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