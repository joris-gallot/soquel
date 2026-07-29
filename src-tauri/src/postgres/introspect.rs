use std::collections::HashMap;

use crate::connectors::{
  ColumnInfo, ForeignKeyInfo, IndexInfo, Introspect, SchemaInfo, SchemaSnapshot, TableInfo,
  TableKind,
};
use crate::error::Error;

use super::PostgresConnection;

// User schemas only: everything except pg_* and information_schema.
const USER_SCHEMAS: &str = "n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'";

#[async_trait::async_trait]
impl Introspect for PostgresConnection {
  async fn schema_snapshot(&self) -> Result<SchemaSnapshot, Error> {
    let tables = format!(
      "SELECT n.nspname, c.relname, c.relkind::text, c.reltuples::float8
       FROM pg_class c
       JOIN pg_namespace n ON n.oid = c.relnamespace
       WHERE c.relkind IN ('r', 'p', 'v', 'm') AND {USER_SCHEMAS}
       ORDER BY n.nspname, c.relname"
    );
    let columns = format!(
      "SELECT n.nspname, c.relname, a.attname,
              format_type(a.atttypid, a.atttypmod),
              NOT a.attnotnull,
              pg_get_expr(d.adbin, d.adrelid)
       FROM pg_attribute a
       JOIN pg_class c ON c.oid = a.attrelid
       JOIN pg_namespace n ON n.oid = c.relnamespace
       LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
       WHERE a.attnum > 0 AND NOT a.attisdropped
         AND c.relkind IN ('r', 'p', 'v', 'm') AND {USER_SCHEMAS}
       ORDER BY n.nspname, c.relname, a.attnum"
    );
    let primary_keys = format!(
      "SELECT n.nspname, c.relname, a.attname
       FROM pg_index i
       JOIN pg_class c ON c.oid = i.indrelid
       JOIN pg_namespace n ON n.oid = c.relnamespace
       JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY (i.indkey)
       WHERE i.indisprimary AND {USER_SCHEMAS}
       ORDER BY n.nspname, c.relname, array_position(i.indkey, a.attnum)"
    );
    let indexes = format!(
      "SELECT n.nspname, c.relname, ic.relname,
              pg_get_indexdef(i.indexrelid), i.indisunique
       FROM pg_index i
       JOIN pg_class c ON c.oid = i.indrelid
       JOIN pg_class ic ON ic.oid = i.indexrelid
       JOIN pg_namespace n ON n.oid = c.relnamespace
       WHERE NOT i.indisprimary AND {USER_SCHEMAS}
       ORDER BY n.nspname, c.relname, ic.relname"
    );
    let foreign_keys = format!(
      "SELECT n.nspname, c.relname, con.conname,
              (SELECT array_agg(a.attname ORDER BY x.ord)
               FROM unnest(con.conkey) WITH ORDINALITY AS x(attnum, ord)
               JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = x.attnum),
              fn.nspname, fc.relname,
              (SELECT array_agg(a.attname ORDER BY x.ord)
               FROM unnest(con.confkey) WITH ORDINALITY AS x(attnum, ord)
               JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = x.attnum)
       FROM pg_constraint con
       JOIN pg_class c ON c.oid = con.conrelid
       JOIN pg_namespace n ON n.oid = c.relnamespace
       JOIN pg_class fc ON fc.oid = con.confrelid
       JOIN pg_namespace fn ON fn.oid = fc.relnamespace
       WHERE con.contype = 'f' AND {USER_SCHEMAS}
       ORDER BY n.nspname, c.relname, con.conname"
    );

    let pg = self.checkout().await?;

    // (schema, table) -> TableInfo, insertion-ordered by the tables query.
    let mut order: Vec<(String, String)> = Vec::new();
    let mut map: HashMap<(String, String), TableInfo> = HashMap::new();

    for row in pg.client.query(&tables, &[]).await? {
      let key = (row.get::<_, String>(0), row.get::<_, String>(1));
      let kind = match row.get::<_, String>(2).as_str() {
        "v" => TableKind::View,
        "m" => TableKind::MaterializedView,
        _ => TableKind::Table,
      };
      order.push(key.clone());
      map.insert(
        key,
        TableInfo {
          name: order.last().unwrap().1.clone(),
          kind,
          estimated_rows: row.get(3),
          columns: Vec::new(),
          primary_key: Vec::new(),
          indexes: Vec::new(),
          foreign_keys: Vec::new(),
        },
      );
    }

    for row in pg.client.query(&columns, &[]).await? {
      if let Some(table) = map.get_mut(&(row.get(0), row.get(1))) {
        table.columns.push(ColumnInfo {
          name: row.get(2),
          data_type: row.get(3),
          nullable: row.get(4),
          default: row.get(5),
        });
      }
    }

    for row in pg.client.query(&primary_keys, &[]).await? {
      if let Some(table) = map.get_mut(&(row.get(0), row.get(1))) {
        table.primary_key.push(row.get(2));
      }
    }

    for row in pg.client.query(&indexes, &[]).await? {
      if let Some(table) = map.get_mut(&(row.get(0), row.get(1))) {
        table.indexes.push(IndexInfo {
          name: row.get(2),
          definition: row.get(3),
          unique: row.get(4),
        });
      }
    }

    for row in pg.client.query(&foreign_keys, &[]).await? {
      if let Some(table) = map.get_mut(&(row.get(0), row.get(1))) {
        table.foreign_keys.push(ForeignKeyInfo {
          name: row.get(2),
          columns: row.get(3),
          referenced_schema: row.get(4),
          referenced_table: row.get(5),
          referenced_columns: row.get(6),
        });
      }
    }

    let mut schemas: Vec<SchemaInfo> = Vec::new();
    for key in order {
      let table = map.remove(&key).unwrap();
      match schemas.last_mut() {
        Some(schema) if schema.name == key.0 => schema.tables.push(table),
        _ => schemas.push(SchemaInfo {
          name: key.0,
          tables: vec![table],
        }),
      }
    }
    Ok(SchemaSnapshot { schemas })
  }
}

#[cfg(test)]
mod tests {
  use super::super::tests::test_connection_from_env;
  use crate::connectors::{Introspect, TableKind};

  #[tokio::test]
  async fn integration_postgres_schema_snapshot() {
    let Some(pg) = test_connection_from_env().await else {
      return;
    };
    let snapshot = pg.schema_snapshot().await.unwrap();

    let names: Vec<&str> = snapshot.schemas.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["app", "public"]);

    let app = &snapshot.schemas[0];
    let customers = app.tables.iter().find(|t| t.name == "customers").unwrap();
    assert_eq!(customers.kind, TableKind::Table);
    assert_eq!(customers.primary_key, vec!["id"]);
    let email = customers
      .columns
      .iter()
      .find(|c| c.name == "email")
      .unwrap();
    assert!(email.nullable);
    assert_eq!(email.data_type, "text");
    let tags = customers.columns.iter().find(|c| c.name == "tags").unwrap();
    assert_eq!(tags.data_type, "text[]");
    let id = customers.columns.iter().find(|c| c.name == "id").unwrap();
    assert!(id.default.as_deref().unwrap().contains("nextval"));

    let orders = app.tables.iter().find(|t| t.name == "orders").unwrap();
    let fk = &orders.foreign_keys[0];
    assert_eq!(fk.columns, vec!["customer_id"]);
    assert_eq!(fk.referenced_schema, "app");
    assert_eq!(fk.referenced_table, "customers");
    assert_eq!(fk.referenced_columns, vec!["id"]);
    assert!(orders
      .indexes
      .iter()
      .any(|i| i.name == "orders_customer_idx" && !i.unique));

    let view = app
      .tables
      .iter()
      .find(|t| t.name == "recent_orders")
      .unwrap();
    assert_eq!(view.kind, TableKind::View);
    assert!(!view.columns.is_empty());
  }
}
