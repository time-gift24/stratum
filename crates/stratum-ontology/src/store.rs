//! Concrete PostgreSQL persistence for Ontology aggregates.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use crate::{
    Canvas, CanvasPosition, Cardinality, CreateOntology, LinkType, LinkTypeId, ListOntologies,
    ListSort, Neighborhood, ObjectType, ObjectTypeId, Ontology, OntologyId, OntologyListPage,
    OntologyRecord, OntologyStoreError, OntologySummary, Property, PropertyId, ValueType, validate,
};

const MAX_BIND_PARAMETERS: usize = 65_000;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Concrete PostgreSQL store for canonical Ontology metadata.
#[derive(Clone)]
pub struct OntologyStore {
    pool: PgPool,
}

impl OntologyStore {
    /// Connects to PostgreSQL and applies this crate's embedded migration.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyStoreError::Connection`] when the pool cannot connect,
    /// or [`OntologyStoreError::Migration`] when the embedded schema migration
    /// cannot complete.
    #[tracing::instrument(skip(database_url), fields(operation = "ontology.connect"))]
    pub async fn connect(database_url: &str) -> Result<Self, OntologyStoreError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|source| OntologyStoreError::Connection { source })?;
        MIGRATOR
            .run(&pool)
            .await
            .map_err(|source| OntologyStoreError::Migration { source })?;
        Ok(Self { pool })
    }

    /// Reports whether PostgreSQL can currently serve a trivial query.
    #[tracing::instrument(skip(self), fields(operation = "ontology.is_ready"))]
    pub async fn is_ready(&self) -> bool {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }

    /// Creates an empty Ontology aggregate.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyStoreError::Validation`] for invalid metadata,
    /// [`OntologyStoreError::NameConflict`] for an existing deployment-wide
    /// name, or a typed persistence error.
    #[tracing::instrument(skip(self, input), fields(operation = "ontology.create"))]
    pub async fn create(
        &self,
        input: CreateOntology,
    ) -> Result<OntologyRecord, OntologyStoreError> {
        let ontology = Ontology {
            id: OntologyId::new(),
            name: input.name,
            display_name: input.display_name,
            description: input.description,
            object_types: Vec::new(),
            link_types: Vec::new(),
            canvas: Canvas {
                positions: Vec::new(),
            },
        };
        validate(&ontology)?;

        let row = sqlx::query_as::<_, RootRow>(
            "INSERT INTO ontologies (id, name, display_name, description, revision, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 1, clock_timestamp(), clock_timestamp()) \
             RETURNING id, name, display_name, description, revision, created_at, updated_at",
        )
        .bind(ontology.id.as_uuid())
        .bind(&ontology.name)
        .bind(&ontology.display_name)
        .bind(&ontology.description)
        .fetch_one(&self.pool)
        .await
        .map_err(map_write_error)?;

        Ok(OntologyRecord {
            ontology,
            revision: row.revision,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    /// Lists Ontology summaries using one deterministic SQL statement.
    ///
    /// # Errors
    ///
    /// Returns a typed persistence error when PostgreSQL cannot complete the
    /// query or returns an invalid persisted identity.
    #[tracing::instrument(skip(self), fields(operation = "ontology.list"))]
    pub async fn list(
        &self,
        request: ListOntologies,
    ) -> Result<OntologyListPage, OntologyStoreError> {
        let offset = i64::from(request.page.saturating_sub(1)) * i64::from(request.per_page);
        let rows = sqlx::query_as::<_, ListRow>(request.sort.sql())
            .bind(i64::from(request.per_page))
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(map_database_error)?;
        let total = rows.first().map_or(0, |row| row.total);
        let mut data = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(summary) = row.into_summary()? {
                data.push(summary);
            }
        }
        Ok(OntologyListPage {
            data,
            page: request.page,
            per_page: request.per_page,
            total,
        })
    }

    /// Reads a complete Ontology from one repeatable read-only snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed persistence error when PostgreSQL cannot complete the
    /// read or persisted data violates canonical invariants.
    #[tracing::instrument(skip(self), fields(operation = "ontology.get", ontology_id = %id))]
    pub async fn get(&self, id: OntologyId) -> Result<Option<OntologyRecord>, OntologyStoreError> {
        let mut transaction = self.begin_read_transaction().await?;
        let result = load_record(&mut transaction, id).await?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(result)
    }

    /// Atomically replaces a complete Ontology document if its revision is current.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyStoreError::Validation`] before beginning a write for
    /// invalid candidates, [`OntologyStoreError::NotFound`] or
    /// [`OntologyStoreError::Stale`] when compare-and-swap fails, or a typed
    /// persistence error. A failed replacement rolls back both revision and
    /// child rows.
    #[tracing::instrument(
        skip(self, candidate, expected_revision),
        fields(operation = "ontology.replace", ontology_id = %candidate.id)
    )]
    pub async fn replace(
        &self,
        candidate: &Ontology,
        expected_revision: i64,
    ) -> Result<i64, OntologyStoreError> {
        validate(candidate)?;
        let mut transaction = self.begin_write_transaction().await?;
        let result = replace_in_transaction(&mut transaction, candidate, expected_revision).await;
        match result {
            Ok(revision) => {
                transaction.commit().await.map_err(map_database_error)?;
                Ok(revision)
            }
            Err(error) => {
                transaction.rollback().await.map_err(map_database_error)?;
                Err(error)
            }
        }
    }

    /// Conditionally deletes an Ontology and every child row.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyStoreError::NotFound`] or [`OntologyStoreError::Stale`]
    /// when the revision condition cannot match, or a typed persistence error.
    #[tracing::instrument(
        skip(self, id, expected_revision),
        fields(operation = "ontology.delete", ontology_id = %id)
    )]
    pub async fn delete(
        &self,
        id: OntologyId,
        expected_revision: i64,
    ) -> Result<(), OntologyStoreError> {
        let mut transaction = self.begin_write_transaction().await?;
        let result = delete_in_transaction(&mut transaction, id, expected_revision).await;
        match result {
            Ok(()) => {
                transaction.commit().await.map_err(map_database_error)?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await.map_err(map_database_error)?;
                Err(error)
            }
        }
    }

    /// Reads a bidirectional induced Object Type subgraph from one snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyStoreError::NotFound`] when the Ontology is absent,
    /// [`OntologyStoreError::ObjectTypeNotFound`] when the origin is not in the
    /// Ontology, or a typed persistence error.
    #[tracing::instrument(
        skip(self),
        fields(
            operation = "ontology.neighborhood",
            ontology_id = %ontology_id,
            origin_object_type_id = %origin_object_type_id,
            depth
        )
    )]
    pub async fn neighborhood(
        &self,
        ontology_id: OntologyId,
        origin_object_type_id: ObjectTypeId,
        depth: u8,
    ) -> Result<Neighborhood, OntologyStoreError> {
        if depth > 5 {
            return Err(OntologyStoreError::InvalidDepth);
        }
        let mut transaction = self.begin_read_transaction().await?;
        let result = neighborhood_in_transaction(
            &mut transaction,
            ontology_id,
            origin_object_type_id,
            depth,
        )
        .await?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(result)
    }

    async fn begin_read_transaction(
        &self,
    ) -> Result<Transaction<'_, Postgres>, OntologyStoreError> {
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        Ok(transaction)
    }

    async fn begin_write_transaction(
        &self,
    ) -> Result<Transaction<'_, Postgres>, OntologyStoreError> {
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED READ WRITE")
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        Ok(transaction)
    }
}

impl ListSort {
    const fn sql(self) -> &'static str {
        match self {
            Self::NameAsc => LIST_BY_NAME_ASC,
            Self::NameDesc => LIST_BY_NAME_DESC,
            Self::DisplayNameAsc => LIST_BY_DISPLAY_NAME_ASC,
            Self::DisplayNameDesc => LIST_BY_DISPLAY_NAME_DESC,
            Self::CreatedAtAsc => LIST_BY_CREATED_AT_ASC,
            Self::CreatedAtDesc => LIST_BY_CREATED_AT_DESC,
            Self::UpdatedAtAsc => LIST_BY_UPDATED_AT_ASC,
            Self::UpdatedAtDesc => LIST_BY_UPDATED_AT_DESC,
        }
    }
}

const LIST_BY_NAME_ASC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY name COLLATE \"C\" ASC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.name COLLATE \"C\" ASC NULLS LAST, page.id ASC NULLS LAST"
);
const LIST_BY_NAME_DESC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY name COLLATE \"C\" DESC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.name COLLATE \"C\" DESC NULLS LAST, page.id ASC NULLS LAST"
);
const LIST_BY_DISPLAY_NAME_ASC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY display_name COLLATE \"C\" ASC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.display_name COLLATE \"C\" ASC NULLS LAST, page.id ASC NULLS LAST"
);
const LIST_BY_DISPLAY_NAME_DESC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY display_name COLLATE \"C\" DESC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.display_name COLLATE \"C\" DESC NULLS LAST, page.id ASC NULLS LAST"
);
const LIST_BY_CREATED_AT_ASC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY created_at ASC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.created_at ASC NULLS LAST, page.id ASC NULLS LAST"
);
const LIST_BY_CREATED_AT_DESC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY created_at DESC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.created_at DESC NULLS LAST, page.id ASC NULLS LAST"
);
const LIST_BY_UPDATED_AT_ASC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY updated_at ASC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.updated_at ASC NULLS LAST, page.id ASC NULLS LAST"
);
const LIST_BY_UPDATED_AT_DESC: &str = concat!(
    "WITH total AS (SELECT count(*)::bigint AS total FROM ontologies), ",
    "page AS (SELECT id, name, display_name, description, created_at, updated_at FROM ontologies ",
    "ORDER BY updated_at DESC, id ASC LIMIT $1 OFFSET $2) ",
    "SELECT page.id, page.name, page.display_name, page.description, page.created_at, page.updated_at, total.total ",
    "FROM total LEFT JOIN page ON TRUE ORDER BY page.updated_at DESC NULLS LAST, page.id ASC NULLS LAST"
);

#[derive(Debug, FromRow)]
struct RootRow {
    id: Uuid,
    name: String,
    display_name: String,
    description: Option<String>,
    revision: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct ListRow {
    id: Option<Uuid>,
    name: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    total: i64,
}

impl ListRow {
    fn into_summary(self) -> Result<Option<OntologySummary>, OntologyStoreError> {
        let Some(id) = self.id else {
            return Ok(None);
        };
        Ok(Some(OntologySummary {
            id: OntologyId::try_from(id).map_err(|_| OntologyStoreError::CorruptData)?,
            name: self.name.ok_or(OntologyStoreError::CorruptData)?,
            display_name: self.display_name.ok_or(OntologyStoreError::CorruptData)?,
            description: self.description,
            created_at: self.created_at.ok_or(OntologyStoreError::CorruptData)?,
            updated_at: self.updated_at.ok_or(OntologyStoreError::CorruptData)?,
        }))
    }
}

#[derive(Debug, FromRow)]
struct ObjectTypeRow {
    id: Uuid,
    name: String,
    display_name: String,
    description: Option<String>,
}

#[derive(Debug, FromRow)]
struct PropertyBatchRow {
    ids: Vec<Uuid>,
    object_type_ids: Vec<Uuid>,
    names: Vec<String>,
    display_names: Vec<String>,
    descriptions: Vec<Option<String>>,
    value_types: Vec<String>,
    required_values: Vec<bool>,
}

#[derive(Debug, FromRow)]
struct LinkTypeRow {
    id: Uuid,
    name: String,
    display_name: String,
    description: Option<String>,
    source_object_type_id: Uuid,
    target_object_type_id: Uuid,
    source_to_target: String,
    target_to_source: String,
}

#[derive(Debug, FromRow)]
struct CanvasPositionRow {
    object_type_id: Uuid,
    x: f64,
    y: f64,
}

#[derive(Debug, FromRow)]
struct NeighborhoodExistenceRow {
    ontology_exists: bool,
    origin_exists: bool,
}

#[derive(Debug, FromRow)]
struct NeighborhoodGraphRow {
    ontology_exists: bool,
    origin_exists: bool,
    object_type_count: i64,
    link_id: Option<Uuid>,
    link_name: Option<String>,
    link_display_name: Option<String>,
    link_description: Option<String>,
    source_object_type_id: Option<Uuid>,
    target_object_type_id: Option<Uuid>,
    source_to_target: Option<String>,
    target_to_source: Option<String>,
}

async fn load_record(
    transaction: &mut Transaction<'_, Postgres>,
    id: OntologyId,
) -> Result<Option<OntologyRecord>, OntologyStoreError> {
    let root = sqlx::query_as::<_, RootRow>(
        "SELECT id, name, display_name, description, revision, created_at, updated_at \
         FROM ontologies WHERE id = $1",
    )
    .bind(id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    let Some(root) = root else {
        return Ok(None);
    };
    let stored_id = OntologyId::try_from(root.id).map_err(|_| OntologyStoreError::CorruptData)?;
    if stored_id != id {
        return Err(OntologyStoreError::CorruptData);
    }
    let (object_types, link_types, canvas) = load_components(transaction, id, None, None).await?;
    Ok(Some(OntologyRecord {
        ontology: Ontology {
            id: stored_id,
            name: root.name,
            display_name: root.display_name,
            description: root.description,
            object_types,
            link_types,
            canvas,
        },
        revision: root.revision,
        created_at: root.created_at,
        updated_at: root.updated_at,
    }))
}

async fn load_components(
    transaction: &mut Transaction<'_, Postgres>,
    ontology_id: OntologyId,
    selected_object_type_ids: Option<&[Uuid]>,
    preloaded_link_rows: Option<Vec<LinkTypeRow>>,
) -> Result<(Vec<ObjectType>, Vec<LinkType>, Canvas), OntologyStoreError> {
    let object_rows = if let Some(ids) = selected_object_type_ids {
        sqlx::query_as::<_, ObjectTypeRow>(
            "SELECT id, name, display_name, description FROM ontology_object_types \
             WHERE ontology_id = $1 AND id = ANY($2) ORDER BY sort_order, id",
        )
        .bind(ontology_id.as_uuid())
        .bind(ids)
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?
    } else {
        sqlx::query_as::<_, ObjectTypeRow>(
            "SELECT id, name, display_name, description FROM ontology_object_types \
             WHERE ontology_id = $1 ORDER BY sort_order, id",
        )
        .bind(ontology_id.as_uuid())
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?
    };

    let mut object_types = Vec::with_capacity(object_rows.len());
    let mut object_indexes = HashMap::with_capacity(object_rows.len());
    let mut object_ids = Vec::with_capacity(object_rows.len());
    for row in object_rows {
        let id = ObjectTypeId::try_from(row.id).map_err(|_| OntologyStoreError::CorruptData)?;
        object_indexes.insert(id, object_types.len());
        object_ids.push(id.as_uuid());
        object_types.push(ObjectType {
            id,
            name: row.name,
            display_name: row.display_name,
            description: row.description,
            properties: Vec::new(),
        });
    }

    if object_ids.is_empty() {
        return Ok((
            object_types,
            Vec::new(),
            Canvas {
                positions: Vec::new(),
            },
        ));
    }

    let property_batch = sqlx::query_as::<_, PropertyBatchRow>(
        "SELECT \
         COALESCE(array_agg(id ORDER BY object_type_id, sort_order, id), ARRAY[]::uuid[]) AS ids, \
         COALESCE(array_agg(object_type_id ORDER BY object_type_id, sort_order, id), ARRAY[]::uuid[]) AS object_type_ids, \
         COALESCE(array_agg(name ORDER BY object_type_id, sort_order, id), ARRAY[]::text[]) AS names, \
         COALESCE(array_agg(display_name ORDER BY object_type_id, sort_order, id), ARRAY[]::text[]) AS display_names, \
         COALESCE(array_agg(description ORDER BY object_type_id, sort_order, id), ARRAY[]::text[]) AS descriptions, \
         COALESCE(array_agg(value_type ORDER BY object_type_id, sort_order, id), ARRAY[]::text[]) AS value_types, \
         COALESCE(array_agg(required ORDER BY object_type_id, sort_order, id), ARRAY[]::boolean[]) AS required_values \
         FROM ontology_properties WHERE object_type_id = ANY($1)",
    )
    .bind(&object_ids)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    let property_count = property_batch.ids.len();
    if [
        property_batch.object_type_ids.len(),
        property_batch.names.len(),
        property_batch.display_names.len(),
        property_batch.descriptions.len(),
        property_batch.value_types.len(),
        property_batch.required_values.len(),
    ]
    .into_iter()
    .any(|length| length != property_count)
    {
        return Err(OntologyStoreError::CorruptData);
    }
    let mut object_type_ids = property_batch.object_type_ids.into_iter();
    let mut names = property_batch.names.into_iter();
    let mut display_names = property_batch.display_names.into_iter();
    let mut descriptions = property_batch.descriptions.into_iter();
    let mut value_types = property_batch.value_types.into_iter();
    let mut required_values = property_batch.required_values.into_iter();
    for id in property_batch.ids {
        let owner = ObjectTypeId::try_from(
            object_type_ids
                .next()
                .ok_or(OntologyStoreError::CorruptData)?,
        )
        .map_err(|_| OntologyStoreError::CorruptData)?;
        let index = *object_indexes
            .get(&owner)
            .ok_or(OntologyStoreError::CorruptData)?;
        let value_type = value_types
            .next()
            .and_then(|value| ValueType::from_database(&value))
            .ok_or(OntologyStoreError::CorruptData)?;
        object_types[index].properties.push(Property {
            id: PropertyId::try_from(id).map_err(|_| OntologyStoreError::CorruptData)?,
            name: names.next().ok_or(OntologyStoreError::CorruptData)?,
            display_name: display_names
                .next()
                .ok_or(OntologyStoreError::CorruptData)?,
            description: descriptions.next().ok_or(OntologyStoreError::CorruptData)?,
            value_type,
            required: required_values
                .next()
                .ok_or(OntologyStoreError::CorruptData)?,
        });
    }

    let filter_preloaded_link_rows =
        preloaded_link_rows.is_some() && selected_object_type_ids.is_some();
    let link_rows = if let Some(link_rows) = preloaded_link_rows {
        link_rows
    } else if selected_object_type_ids.is_some() {
        sqlx::query_as::<_, LinkTypeRow>(
            "SELECT id, name, display_name, description, source_object_type_id, \
             target_object_type_id, source_to_target, target_to_source \
             FROM ontology_link_types WHERE ontology_id = $1 \
             AND source_object_type_id = ANY($2) AND target_object_type_id = ANY($2) \
             ORDER BY sort_order, id",
        )
        .bind(ontology_id.as_uuid())
        .bind(&object_ids)
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?
    } else {
        sqlx::query_as::<_, LinkTypeRow>(
            "SELECT id, name, display_name, description, source_object_type_id, \
             target_object_type_id, source_to_target, target_to_source \
             FROM ontology_link_types WHERE ontology_id = $1 ORDER BY sort_order, id",
        )
        .bind(ontology_id.as_uuid())
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?
    };
    let selected_link_endpoints =
        filter_preloaded_link_rows.then(|| object_ids.iter().copied().collect::<HashSet<_>>());
    let mut link_types = Vec::with_capacity(link_rows.len());
    for row in link_rows {
        if selected_link_endpoints.as_ref().is_some_and(|selected| {
            !selected.contains(&row.source_object_type_id)
                || !selected.contains(&row.target_object_type_id)
        }) {
            continue;
        }
        link_types.push(LinkType {
            id: LinkTypeId::try_from(row.id).map_err(|_| OntologyStoreError::CorruptData)?,
            name: row.name,
            display_name: row.display_name,
            description: row.description,
            source_object_type_id: ObjectTypeId::try_from(row.source_object_type_id)
                .map_err(|_| OntologyStoreError::CorruptData)?,
            target_object_type_id: ObjectTypeId::try_from(row.target_object_type_id)
                .map_err(|_| OntologyStoreError::CorruptData)?,
            source_to_target: Cardinality::from_database(&row.source_to_target)
                .ok_or(OntologyStoreError::CorruptData)?,
            target_to_source: Cardinality::from_database(&row.target_to_source)
                .ok_or(OntologyStoreError::CorruptData)?,
        });
    }

    let position_rows = sqlx::query_as::<_, CanvasPositionRow>(
        "SELECT object_type_id, x, y FROM ontology_canvas_positions \
         WHERE object_type_id = ANY($1) ORDER BY sort_order, object_type_id",
    )
    .bind(&object_ids)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    let mut positions = Vec::with_capacity(position_rows.len());
    for row in position_rows {
        positions.push(CanvasPosition {
            object_type_id: ObjectTypeId::try_from(row.object_type_id)
                .map_err(|_| OntologyStoreError::CorruptData)?,
            x: row.x,
            y: row.y,
        });
    }
    Ok((object_types, link_types, Canvas { positions }))
}

async fn replace_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    candidate: &Ontology,
    expected_revision: i64,
) -> Result<i64, OntologyStoreError> {
    let revision = sqlx::query_scalar::<_, i64>(
        "UPDATE ontologies SET name = $2, display_name = $3, description = $4, \
         revision = revision + 1, updated_at = clock_timestamp() \
         WHERE id = $1 AND revision = $5 RETURNING revision",
    )
    .bind(candidate.id.as_uuid())
    .bind(&candidate.name)
    .bind(&candidate.display_name)
    .bind(&candidate.description)
    .bind(expected_revision)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_write_error)?;
    let Some(revision) = revision else {
        return conditional_failure(transaction, candidate.id).await;
    };

    for statement in [
        "DELETE FROM ontology_link_types WHERE ontology_id = $1",
        "DELETE FROM ontology_properties WHERE object_type_id IN \
         (SELECT id FROM ontology_object_types WHERE ontology_id = $1)",
        "DELETE FROM ontology_canvas_positions WHERE object_type_id IN \
         (SELECT id FROM ontology_object_types WHERE ontology_id = $1)",
        "DELETE FROM ontology_object_types WHERE ontology_id = $1",
    ] {
        sqlx::query(statement)
            .bind(candidate.id.as_uuid())
            .execute(&mut **transaction)
            .await
            .map_err(map_database_error)?;
    }
    insert_children(transaction, candidate).await?;
    Ok(revision)
}

async fn delete_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    id: OntologyId,
    expected_revision: i64,
) -> Result<(), OntologyStoreError> {
    let deleted = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM ontologies WHERE id = $1 AND revision = $2 RETURNING id",
    )
    .bind(id.as_uuid())
    .bind(expected_revision)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    if deleted.is_some() {
        Ok(())
    } else {
        conditional_failure(transaction, id).await
    }
}

async fn conditional_failure<T>(
    transaction: &mut Transaction<'_, Postgres>,
    id: OntologyId,
) -> Result<T, OntologyStoreError> {
    let exists = sqlx::query_scalar::<_, Uuid>("SELECT id FROM ontologies WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(map_database_error)?;
    if exists.is_some() {
        Err(OntologyStoreError::Stale)
    } else {
        Err(OntologyStoreError::NotFound)
    }
}

async fn neighborhood_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    ontology_id: OntologyId,
    origin_object_type_id: ObjectTypeId,
    depth: u8,
) -> Result<Neighborhood, OntologyStoreError> {
    let mut visited = HashSet::new();
    visited.insert(origin_object_type_id.as_uuid());
    let mut all_object_types_selected = false;
    if depth == 0 {
        let existence = sqlx::query_as::<_, NeighborhoodExistenceRow>(
            "SELECT EXISTS (SELECT 1 FROM ontologies WHERE id = $1) AS ontology_exists, \
             EXISTS (SELECT 1 FROM ontology_object_types WHERE ontology_id = $1 AND id = $2) \
             AS origin_exists",
        )
        .bind(ontology_id.as_uuid())
        .bind(origin_object_type_id.as_uuid())
        .fetch_one(&mut **transaction)
        .await
        .map_err(map_database_error)?;
        if !existence.ontology_exists {
            return Err(OntologyStoreError::NotFound);
        }
        if !existence.origin_exists {
            return Err(OntologyStoreError::ObjectTypeNotFound);
        }
    } else {
        let graph_rows = sqlx::query_as::<_, NeighborhoodGraphRow>(
            "WITH status AS ( \
             SELECT EXISTS (SELECT 1 FROM ontologies WHERE id = $1) AS ontology_exists, \
             EXISTS (SELECT 1 FROM ontology_object_types WHERE ontology_id = $1 AND id = $2) \
             AS origin_exists, \
             (SELECT count(*) FROM ontology_object_types WHERE ontology_id = $1) \
             AS object_type_count) \
             SELECT status.ontology_exists, status.origin_exists, status.object_type_count, \
             links.id AS link_id, links.name AS link_name, \
             links.display_name AS link_display_name, links.description AS link_description, \
             links.source_object_type_id, links.target_object_type_id, \
             links.source_to_target, links.target_to_source \
             FROM status LEFT JOIN ontology_link_types AS links ON links.ontology_id = $1 \
             ORDER BY links.sort_order, links.id",
        )
        .bind(ontology_id.as_uuid())
        .bind(origin_object_type_id.as_uuid())
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?;
        let graph = graph_rows.first().ok_or(OntologyStoreError::CorruptData)?;
        if !graph.ontology_exists {
            return Err(OntologyStoreError::NotFound);
        }
        if !graph.origin_exists {
            return Err(OntologyStoreError::ObjectTypeNotFound);
        }
        let object_type_count = usize::try_from(graph.object_type_count)
            .map_err(|_| OntologyStoreError::CorruptData)?;
        let mut link_rows = Vec::with_capacity(graph_rows.len());
        for row in graph_rows {
            let Some(id) = row.link_id else {
                continue;
            };
            link_rows.push(LinkTypeRow {
                id,
                name: row.link_name.ok_or(OntologyStoreError::CorruptData)?,
                display_name: row
                    .link_display_name
                    .ok_or(OntologyStoreError::CorruptData)?,
                description: row.link_description,
                source_object_type_id: row
                    .source_object_type_id
                    .ok_or(OntologyStoreError::CorruptData)?,
                target_object_type_id: row
                    .target_object_type_id
                    .ok_or(OntologyStoreError::CorruptData)?,
                source_to_target: row
                    .source_to_target
                    .ok_or(OntologyStoreError::CorruptData)?,
                target_to_source: row
                    .target_to_source
                    .ok_or(OntologyStoreError::CorruptData)?,
            });
        }

        visited.reserve(object_type_count.saturating_sub(1));
        let mut frontier = HashSet::with_capacity(1);
        frontier.insert(origin_object_type_id.as_uuid());
        for _ in 0..depth {
            let mut next = HashSet::new();
            for link in &link_rows {
                let neighbor = if frontier.contains(&link.source_object_type_id) {
                    Some(link.target_object_type_id)
                } else if frontier.contains(&link.target_object_type_id) {
                    Some(link.source_object_type_id)
                } else {
                    None
                };
                if let Some(id) = neighbor
                    && visited.insert(id)
                {
                    next.insert(id);
                }
            }
            frontier = next;
            all_object_types_selected = visited.len() == object_type_count;
            if frontier.is_empty() || all_object_types_selected {
                break;
            }
        }

        let selected = visited.into_iter().collect::<Vec<_>>();
        let selected_object_type_ids = if all_object_types_selected {
            None
        } else {
            Some(selected.as_slice())
        };
        let (object_types, link_types, canvas) = load_components(
            transaction,
            ontology_id,
            selected_object_type_ids,
            Some(link_rows),
        )
        .await?;
        return Ok(Neighborhood {
            origin_object_type_id,
            depth,
            object_types,
            link_types,
            canvas,
        });
    }

    let selected = visited.into_iter().collect::<Vec<_>>();
    let selected_object_type_ids = if all_object_types_selected {
        None
    } else {
        Some(selected.as_slice())
    };
    let (object_types, link_types, canvas) =
        load_components(transaction, ontology_id, selected_object_type_ids, None).await?;
    Ok(Neighborhood {
        origin_object_type_id,
        depth,
        object_types,
        link_types,
        canvas,
    })
}

async fn insert_children(
    transaction: &mut Transaction<'_, Postgres>,
    candidate: &Ontology,
) -> Result<(), OntologyStoreError> {
    const OBJECT_TYPE_COLUMNS: usize = 6;
    const PROPERTY_COLUMNS: usize = 8;
    const LINK_TYPE_COLUMNS: usize = 10;
    const POSITION_COLUMNS: usize = 4;

    let object_chunk_size = MAX_BIND_PARAMETERS / OBJECT_TYPE_COLUMNS;
    let object_orders = sequence_orders(candidate.object_types.len())?;
    for (chunk, sort_orders) in candidate
        .object_types
        .chunks(object_chunk_size)
        .zip(object_orders.chunks(object_chunk_size))
    {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO ontology_object_types \
             (id, ontology_id, name, display_name, description, sort_order) ",
        );
        query.push_values(
            chunk.iter().zip(sort_orders),
            |mut values, (object_type, sort_order)| {
                values
                    .push_bind(object_type.id.as_uuid())
                    .push_bind(candidate.id.as_uuid())
                    .push_bind(&object_type.name)
                    .push_bind(&object_type.display_name)
                    .push_bind(&object_type.description)
                    .push_bind(*sort_order);
            },
        );
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(map_write_error)?;
    }

    let property_chunk_size = MAX_BIND_PARAMETERS / PROPERTY_COLUMNS;
    for object_type in &candidate.object_types {
        let property_orders = sequence_orders(object_type.properties.len())?;
        for (chunk, sort_orders) in object_type
            .properties
            .chunks(property_chunk_size)
            .zip(property_orders.chunks(property_chunk_size))
        {
            let mut query = QueryBuilder::<Postgres>::new(
                "INSERT INTO ontology_properties \
                 (id, object_type_id, name, display_name, description, value_type, required, sort_order) ",
            );
            query.push_values(
                chunk.iter().zip(sort_orders),
                |mut values, (property, sort_order)| {
                    values
                        .push_bind(property.id.as_uuid())
                        .push_bind(object_type.id.as_uuid())
                        .push_bind(&property.name)
                        .push_bind(&property.display_name)
                        .push_bind(&property.description)
                        .push_bind(property.value_type.as_str())
                        .push_bind(property.required)
                        .push_bind(*sort_order);
                },
            );
            query
                .build()
                .execute(&mut **transaction)
                .await
                .map_err(map_write_error)?;
        }
    }

    let link_chunk_size = MAX_BIND_PARAMETERS / LINK_TYPE_COLUMNS;
    let link_orders = sequence_orders(candidate.link_types.len())?;
    for (chunk, sort_orders) in candidate
        .link_types
        .chunks(link_chunk_size)
        .zip(link_orders.chunks(link_chunk_size))
    {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO ontology_link_types \
             (id, ontology_id, name, display_name, description, source_object_type_id, \
             target_object_type_id, source_to_target, target_to_source, sort_order) ",
        );
        query.push_values(
            chunk.iter().zip(sort_orders),
            |mut values, (link_type, sort_order)| {
                values
                    .push_bind(link_type.id.as_uuid())
                    .push_bind(candidate.id.as_uuid())
                    .push_bind(&link_type.name)
                    .push_bind(&link_type.display_name)
                    .push_bind(&link_type.description)
                    .push_bind(link_type.source_object_type_id.as_uuid())
                    .push_bind(link_type.target_object_type_id.as_uuid())
                    .push_bind(link_type.source_to_target.as_str())
                    .push_bind(link_type.target_to_source.as_str())
                    .push_bind(*sort_order);
            },
        );
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(map_write_error)?;
    }

    let position_chunk_size = MAX_BIND_PARAMETERS / POSITION_COLUMNS;
    let position_orders = sequence_orders(candidate.canvas.positions.len())?;
    for (chunk, sort_orders) in candidate
        .canvas
        .positions
        .chunks(position_chunk_size)
        .zip(position_orders.chunks(position_chunk_size))
    {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO ontology_canvas_positions (object_type_id, x, y, sort_order) ",
        );
        query.push_values(
            chunk.iter().zip(sort_orders),
            |mut values, (position, sort_order)| {
                values
                    .push_bind(position.object_type_id.as_uuid())
                    .push_bind(position.x)
                    .push_bind(position.y)
                    .push_bind(*sort_order);
            },
        );
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(map_write_error)?;
    }
    Ok(())
}

fn sequence_orders(count: usize) -> Result<Vec<i32>, OntologyStoreError> {
    (0..count)
        .map(|index| i32::try_from(index).map_err(|_| OntologyStoreError::CorruptData))
        .collect()
}

fn map_write_error(error: sqlx::Error) -> OntologyStoreError {
    if let sqlx::Error::Database(database) = &error
        && database.code().as_deref() == Some("23505")
    {
        return match database.constraint() {
            Some("ontologies_name_key") => OntologyStoreError::NameConflict { source: error },
            Some(
                "ontology_object_types_pkey"
                | "ontology_properties_pkey"
                | "ontology_link_types_pkey",
            ) => OntologyStoreError::EntityIdConflict { source: error },
            _ => map_database_error(error),
        };
    }
    map_database_error(error)
}

fn map_database_error(error: sqlx::Error) -> OntologyStoreError {
    let unavailable = match &error {
        sqlx::Error::Io(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => true,
        sqlx::Error::Database(database) => database
            .code()
            .is_some_and(|code| is_unavailable_sqlstate(code.as_ref())),
        _ => false,
    };
    if unavailable {
        OntologyStoreError::Unavailable { source: error }
    } else {
        OntologyStoreError::Database { source: error }
    }
}

fn is_unavailable_sqlstate(code: &str) -> bool {
    code.starts_with("08") || matches!(code, "57P01" | "57P02" | "57P03")
}

#[cfg(test)]
mod tests {
    use super::is_unavailable_sqlstate;

    #[test]
    fn connection_and_shutdown_sqlstates_are_unavailable() {
        for code in ["08000", "08006", "57P01", "57P02", "57P03"] {
            assert!(
                is_unavailable_sqlstate(code),
                "{code} should be unavailable"
            );
        }
        for code in ["57014", "23505"] {
            assert!(
                !is_unavailable_sqlstate(code),
                "{code} should not be unavailable"
            );
        }
    }
}
