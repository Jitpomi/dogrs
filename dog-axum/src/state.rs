use std::sync::Arc;

use dog_core::DogApp;

pub struct DogAxumState<R, P>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    pub app: Arc<DogApp<R, P>>,
}

impl<R, P> Clone for DogAxumState<R, P>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
        }
    }
}

impl<R, P> DogAxumState<R, P>
where
    R: Send + Sync + 'static,
    P: Send + Sync + Clone + 'static,
{
    pub fn new(app: DogApp<R, P>) -> Self {
        Self { app: Arc::new(app) }
    }
}
