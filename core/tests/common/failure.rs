use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

pub struct Failure<R, E>
where
    R: 'static,
    E: 'static,
{
    inner: Arc<Mutex<Option<Box<dyn FailureProvider<R, E>>>>>,
}

impl<R, E> Clone for Failure<R, E>
where
    R: 'static,
    E: 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<R, E> Default for Failure<R, E>
where
    R: 'static,
    E: 'static,
{
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<R, E> Failure<R, E>
where
    R: 'static,
    E: 'static,
{
    pub fn new<FP>(f: FP) -> Self
    where
        FP: FailureProvider<R, E> + 'static,
    {
        Self {
            inner: Arc::new(Mutex::new(Some(Box::new(f)))),
        }
    }

    //noinspection ALL
    pub fn is_failure(&self, request: R) -> Option<E> {
        if let Some(f) = &mut (*self.inner.lock().unwrap()) {
            f.is_failure(request)
        } else {
            None
        }
    }

    pub fn failed(&self, request: R) -> Result<(), E> {
        match self.is_failure(request) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

pub trait FailureProvider<R, E>: Send {
    //noinspection ALL
    fn is_failure(&mut self, request: R) -> Option<E>;
}

pub struct Function<F, R, E>(F, PhantomData<(R, E)>)
where
    F: Fn(R) -> Option<E> + Send;

impl<F, R, E> Function<F, R, E>
where
    F: Fn(R) -> Option<E> + Send,
{
    pub fn new(from: F) -> Self {
        Self(from, PhantomData::default())
    }
}

#[inline]
pub fn func<F, R, E>(from: F) -> Failure<R, E>
where
    F: Fn(R) -> Option<E> + Send + 'static,
    R: 'static + Send,
    E: 'static + Send,
{
    Failure::new(Function::new(from))
}

impl<F, R, E> From<F> for Function<F, R, E>
where
    F: Fn(R) -> Option<E> + Send,
{
    fn from(f: F) -> Self {
        Self::new(f)
    }
}

impl<F, R, E> FailureProvider<R, E> for Function<F, R, E>
where
    F: Fn(R) -> Option<E> + Send,
    R: Send,
    E: Send,
{
    fn is_failure(&mut self, request: R) -> Option<E> {
        (self.0)(request)
    }
}

pub struct Iter<I, E>(pub I)
where
    I: Iterator<Item = Option<E>>;

impl<I, E> Iter<I, E>
where
    I: Iterator<Item = Option<E>> + Send,
{
    pub fn new<F>(from: F) -> Self
    where
        F: IntoIterator<Item = Option<E>, IntoIter = I>,
    {
        Self(from.into_iter())
    }
}

pub fn iter<F, R, E>(from: F) -> Failure<R, E>
where
    F: IntoIterator<Item = Option<E>> + 'static,
    <F as IntoIterator>::IntoIter: Send,
{
    Failure::new(Iter::new(from))
}

impl<F, I, E> From<F> for Iter<I, E>
where
    I: Iterator<Item = Option<E>> + Send,
    F: IntoIterator<Item = Option<E>, IntoIter = I>,
{
    fn from(i: F) -> Self {
        Self::new(i)
    }
}

impl<I, R, E> FailureProvider<R, E> for Iter<I, E>
where
    I: Iterator<Item = Option<E>> + Send,
{
    fn is_failure(&mut self, _request: R) -> Option<E> {
        self.0.next().flatten()
    }
}
