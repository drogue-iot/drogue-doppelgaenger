use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use tokio_postgres::types::WasNull;
use tokio_postgres::{row::RowIndex, types::Json, Row};

/// Convert a JSON map column from a row into a map value, handling "null" values
/// by using the default value.
///
/// FIXME: Ripped from Drogue IoT Cloud.
pub(crate) fn row_to_map<I>(
    row: &Row,
    idx: I,
) -> Result<BTreeMap<String, String>, tokio_postgres::Error>
where
    I: RowIndex + fmt::Display,
{
    Ok(row
        .try_get::<_, Json<_>>(idx)
        .or_else(|err| fix_null(err, || Json(Default::default())))?
        .0)
}

/// Fix a null error by using an alternative value.
///
/// FIXME: Ripped from Drogue IoT Cloud.
fn fix_null<T, F>(err: tokio_postgres::Error, f: F) -> Result<T, tokio_postgres::Error>
where
    F: FnOnce() -> T,
{
    err.source()
        .and_then(|e| e.downcast_ref::<WasNull>().map(|_| Ok(f())))
        .unwrap_or(Err(err))
}
