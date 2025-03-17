use crate::Result;
use sea_query::{Expr, Iden, OnConflict, Order, Query, SqliteQueryBuilder};
use sea_query_rusqlite::{RusqliteBinder, RusqliteValues};

#[derive(Iden)]
#[iden(rename = "store")]
pub(crate) enum StoreId {
    Table,
    Id,
    Name,
    Value,
    Size,
    TypeHint,
    CreatedAt,
    UpdatedAt,
}

pub(crate) fn stmt_delete_value_by_name<S>(name: S) -> (Box<str>, RusqliteValues)
where
    S: AsRef<str>,
{
    let (sql, values) = Query::delete()
        .from_table(StoreId::Table)
        .cond_where(Expr::column(StoreId::Name).eq(name.as_ref()))
        .build_rusqlite(SqliteQueryBuilder);
    (sql.into_boxed_str(), values)
}

pub(crate) fn stmt_get_all_values() -> (Box<str>, RusqliteValues) {
    let (sql, values) = Query::select()
        .columns([
            StoreId::Name,
            StoreId::Size,
            StoreId::TypeHint,
            StoreId::CreatedAt,
            StoreId::UpdatedAt,
        ])
        .from(StoreId::Table)
        .order_by(StoreId::Id, Order::Desc)
        .build_rusqlite(SqliteQueryBuilder);
    (sql.into_boxed_str(), values)
}

pub(crate) fn stmt_get_value_by_name<S>(name: S) -> (Box<str>, RusqliteValues)
where
    S: AsRef<str>,
{
    let (sql, values) = Query::select()
        .columns([StoreId::TypeHint, StoreId::Value])
        .from(StoreId::Table)
        .cond_where(Expr::column(StoreId::Name).eq(name.as_ref()))
        .build_rusqlite(SqliteQueryBuilder);
    (sql.into_boxed_str(), values)
}

pub(crate) fn stmt_upsert_store<S, T>(
    name: S,
    value: Vec<u8>,
    size: usize,
    type_hint: T,
) -> Result<(Box<str>, RusqliteValues)>
where
    S: AsRef<str>,
    T: AsRef<str>,
{
    let size = u64::try_from(size)?;
    let (sql, values) = Query::insert()
        .into_table(StoreId::Table)
        .columns([
            StoreId::Name,
            StoreId::Value,
            StoreId::Size,
            StoreId::TypeHint,
        ])
        .values_panic([
            name.as_ref().into(),
            value.into(),
            size.into(),
            type_hint.as_ref().into(),
        ])
        .on_conflict(
            OnConflict::column(StoreId::Name)
                .value(StoreId::UpdatedAt, Expr::current_timestamp())
                .update_columns([StoreId::Value, StoreId::Size, StoreId::TypeHint])
                .to_owned(),
        )
        .build_rusqlite(SqliteQueryBuilder);
    Ok((sql.into_boxed_str(), values))
}
