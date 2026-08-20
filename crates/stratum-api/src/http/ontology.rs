//! HTTP DTOs and handlers for canonical Ontology metadata.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    handler::Handler,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ETAG, IF_MATCH, LOCATION},
    },
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use stratum_ontology::{
    Canvas, CanvasPosition, Cardinality, CreateOntology, LinkType, LinkTypeId, ListOntologies,
    ListSort, Neighborhood, ObjectType, ObjectTypeId, Ontology, OntologyId, OntologyListPage,
    OntologyRecord, OntologyStoreError, OntologySummary, Property, PropertyId, ValueType,
};
use tracing::{Span, field};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    error::{ApiError, ErrorKind, ErrorResponse, OntologyValidationErrorResponse},
    state::AppState,
};

const ONTOLOGY_BODY_LIMIT: usize = 2 * 1024 * 1024;
const NEVER_MATCHING_REVISION: i64 = i64::MIN;

macro_rules! id_dto {
    ($dto:ident, $domain:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
        #[serde(transparent)]
        #[schema(
            value_type = String,
            format = Uuid,
            pattern = "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-7[0-9a-fA-F]{3}-[89aAbB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$"
        )]
        pub(crate) struct $dto(Uuid);

        impl TryFrom<$dto> for $domain {
            type Error = stratum_ontology::IdParseError;

            fn try_from(value: $dto) -> Result<Self, Self::Error> {
                Self::try_from(value.0)
            }
        }

        impl From<$domain> for $dto {
            fn from(value: $domain) -> Self {
                Self(value.as_uuid())
            }
        }
    };
}

id_dto!(OntologyIdDto, OntologyId);
id_dto!(ObjectTypeIdDto, ObjectTypeId);
id_dto!(PropertyIdDto, PropertyId);
id_dto!(LinkTypeIdDto, LinkTypeId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValueTypeDto {
    String,
    Integer,
    Number,
    Boolean,
    Date,
    DateTime,
}

impl From<ValueType> for ValueTypeDto {
    fn from(value: ValueType) -> Self {
        match value {
            ValueType::String => Self::String,
            ValueType::Integer => Self::Integer,
            ValueType::Number => Self::Number,
            ValueType::Boolean => Self::Boolean,
            ValueType::Date => Self::Date,
            ValueType::DateTime => Self::DateTime,
        }
    }
}

impl From<ValueTypeDto> for ValueType {
    fn from(value: ValueTypeDto) -> Self {
        match value {
            ValueTypeDto::String => Self::String,
            ValueTypeDto::Integer => Self::Integer,
            ValueTypeDto::Number => Self::Number,
            ValueTypeDto::Boolean => Self::Boolean,
            ValueTypeDto::Date => Self::Date,
            ValueTypeDto::DateTime => Self::DateTime,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CardinalityDto {
    One,
    Many,
}

impl From<Cardinality> for CardinalityDto {
    fn from(value: Cardinality) -> Self {
        match value {
            Cardinality::One => Self::One,
            Cardinality::Many => Self::Many,
        }
    }
}

impl From<CardinalityDto> for Cardinality {
    fn from(value: CardinalityDto) -> Self {
        match value {
            CardinalityDto::One => Self::One,
            CardinalityDto::Many => Self::Many,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct CreateOntologyRequest {
    #[schema(pattern = "^[a-z][a-z0-9_]{0,63}$", max_length = 64)]
    name: String,
    #[schema(min_length = 1, max_length = 200)]
    display_name: String,
    #[serde(
        default,
        deserialize_with = "deserialize_description",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(nullable = false, min_length = 1, max_length = 2000)]
    description: Option<String>,
}

impl From<CreateOntologyRequest> for CreateOntology {
    fn from(value: CreateOntologyRequest) -> Self {
        Self {
            name: value.name,
            display_name: value.display_name,
            description: value.description,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct OntologyDto {
    id: OntologyIdDto,
    #[schema(pattern = "^[a-z][a-z0-9_]{0,63}$", max_length = 64)]
    name: String,
    #[schema(min_length = 1, max_length = 200)]
    display_name: String,
    #[serde(
        default,
        deserialize_with = "deserialize_description",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(nullable = false, min_length = 1, max_length = 2000)]
    description: Option<String>,
    #[schema(max_items = 500)]
    object_types: Vec<ObjectTypeDto>,
    #[schema(max_items = 2000)]
    link_types: Vec<LinkTypeDto>,
    canvas: CanvasDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct ObjectTypeDto {
    id: ObjectTypeIdDto,
    #[schema(pattern = "^[a-z][a-z0-9_]{0,63}$", max_length = 64)]
    name: String,
    #[schema(min_length = 1, max_length = 200)]
    display_name: String,
    #[serde(
        default,
        deserialize_with = "deserialize_description",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(nullable = false, min_length = 1, max_length = 2000)]
    description: Option<String>,
    #[schema(max_items = 100)]
    properties: Vec<PropertyDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct PropertyDto {
    id: PropertyIdDto,
    #[schema(pattern = "^[a-z][a-z0-9_]{0,63}$", max_length = 64)]
    name: String,
    #[schema(min_length = 1, max_length = 200)]
    display_name: String,
    #[serde(
        default,
        deserialize_with = "deserialize_description",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(nullable = false, min_length = 1, max_length = 2000)]
    description: Option<String>,
    value_type: ValueTypeDto,
    required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct LinkTypeDto {
    id: LinkTypeIdDto,
    #[schema(pattern = "^[a-z][a-z0-9_]{0,63}$", max_length = 64)]
    name: String,
    #[schema(min_length = 1, max_length = 200)]
    display_name: String,
    #[serde(
        default,
        deserialize_with = "deserialize_description",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(nullable = false, min_length = 1, max_length = 2000)]
    description: Option<String>,
    source_object_type_id: ObjectTypeIdDto,
    target_object_type_id: ObjectTypeIdDto,
    source_to_target: CardinalityDto,
    target_to_source: CardinalityDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct CanvasDto {
    #[schema(max_items = 500)]
    positions: Vec<CanvasPositionDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct CanvasPositionDto {
    object_type_id: ObjectTypeIdDto,
    x: f64,
    y: f64,
}

impl From<Ontology> for OntologyDto {
    fn from(value: Ontology) -> Self {
        Self {
            id: value.id.into(),
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            object_types: value.object_types.into_iter().map(Into::into).collect(),
            link_types: value.link_types.into_iter().map(Into::into).collect(),
            canvas: value.canvas.into(),
        }
    }
}

impl TryFrom<OntologyDto> for Ontology {
    type Error = ApiError;

    fn try_from(value: OntologyDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value
                .id
                .try_into()
                .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?,
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            object_types: value
                .object_types
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            link_types: value
                .link_types
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            canvas: value.canvas.try_into()?,
        })
    }
}

impl From<ObjectType> for ObjectTypeDto {
    fn from(value: ObjectType) -> Self {
        Self {
            id: value.id.into(),
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            properties: value.properties.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<ObjectTypeDto> for ObjectType {
    type Error = ApiError;

    fn try_from(value: ObjectTypeDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value
                .id
                .try_into()
                .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?,
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            properties: value
                .properties
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl From<Property> for PropertyDto {
    fn from(value: Property) -> Self {
        Self {
            id: value.id.into(),
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            value_type: value.value_type.into(),
            required: value.required,
        }
    }
}

impl TryFrom<PropertyDto> for Property {
    type Error = ApiError;

    fn try_from(value: PropertyDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value
                .id
                .try_into()
                .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?,
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            value_type: value.value_type.into(),
            required: value.required,
        })
    }
}

impl From<LinkType> for LinkTypeDto {
    fn from(value: LinkType) -> Self {
        Self {
            id: value.id.into(),
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            source_object_type_id: value.source_object_type_id.into(),
            target_object_type_id: value.target_object_type_id.into(),
            source_to_target: value.source_to_target.into(),
            target_to_source: value.target_to_source.into(),
        }
    }
}

impl TryFrom<LinkTypeDto> for LinkType {
    type Error = ApiError;

    fn try_from(value: LinkTypeDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value
                .id
                .try_into()
                .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?,
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            source_object_type_id: value
                .source_object_type_id
                .try_into()
                .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?,
            target_object_type_id: value
                .target_object_type_id
                .try_into()
                .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?,
            source_to_target: value.source_to_target.into(),
            target_to_source: value.target_to_source.into(),
        })
    }
}

impl From<Canvas> for CanvasDto {
    fn from(value: Canvas) -> Self {
        Self {
            positions: value.positions.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<CanvasDto> for Canvas {
    type Error = ApiError;

    fn try_from(value: CanvasDto) -> Result<Self, Self::Error> {
        Ok(Self {
            positions: value
                .positions
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl From<CanvasPosition> for CanvasPositionDto {
    fn from(value: CanvasPosition) -> Self {
        Self {
            object_type_id: value.object_type_id.into(),
            x: value.x,
            y: value.y,
        }
    }
}

impl TryFrom<CanvasPositionDto> for CanvasPosition {
    type Error = ApiError;

    fn try_from(value: CanvasPositionDto) -> Result<Self, Self::Error> {
        Ok(Self {
            object_type_id: value
                .object_type_id
                .try_into()
                .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?,
            x: value.x,
            y: value.y,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OntologySummaryDto {
    id: OntologyIdDto,
    name: String,
    display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false, min_length = 1, max_length = 2000)]
    description: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<OntologySummary> for OntologySummaryDto {
    fn from(value: OntologySummary) -> Self {
        Self {
            id: value.id.into(),
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) struct PaginationDto {
    #[schema(minimum = 1)]
    page: u32,
    #[schema(minimum = 1, maximum = 100)]
    per_page: u16,
    #[schema(minimum = 0)]
    total: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OntologyListResponse {
    #[schema(max_items = 100)]
    data: Vec<OntologySummaryDto>,
    pagination: PaginationDto,
}

impl From<OntologyListPage> for OntologyListResponse {
    fn from(value: OntologyListPage) -> Self {
        Self {
            data: value.data.into_iter().map(Into::into).collect(),
            pagination: PaginationDto {
                page: value.page,
                per_page: value.per_page,
                total: value.total,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) struct NeighborhoodDto {
    origin_object_type_id: ObjectTypeIdDto,
    #[schema(minimum = 0, maximum = 5)]
    depth: u8,
    #[schema(max_items = 500)]
    object_types: Vec<ObjectTypeDto>,
    #[schema(max_items = 2000)]
    link_types: Vec<LinkTypeDto>,
    canvas: CanvasDto,
}

impl From<Neighborhood> for NeighborhoodDto {
    fn from(value: Neighborhood) -> Self {
        Self {
            origin_object_type_id: value.origin_object_type_id.into(),
            depth: value.depth,
            object_types: value.object_types.into_iter().map(Into::into).collect(),
            link_types: value.link_types.into_iter().map(Into::into).collect(),
            canvas: value.canvas.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
pub(crate) enum ListSortDto {
    #[serde(rename = "name")]
    NameAsc,
    #[serde(rename = "-name")]
    NameDesc,
    #[serde(rename = "display_name")]
    DisplayNameAsc,
    #[serde(rename = "-display_name")]
    DisplayNameDesc,
    #[serde(rename = "created_at")]
    CreatedAtAsc,
    #[serde(rename = "-created_at")]
    CreatedAtDesc,
    #[serde(rename = "updated_at")]
    UpdatedAtAsc,
    #[serde(rename = "-updated_at")]
    UpdatedAtDesc,
}

impl From<ListSortDto> for ListSort {
    fn from(value: ListSortDto) -> Self {
        match value {
            ListSortDto::NameAsc => Self::NameAsc,
            ListSortDto::NameDesc => Self::NameDesc,
            ListSortDto::DisplayNameAsc => Self::DisplayNameAsc,
            ListSortDto::DisplayNameDesc => Self::DisplayNameDesc,
            ListSortDto::CreatedAtAsc => Self::CreatedAtAsc,
            ListSortDto::CreatedAtDesc => Self::CreatedAtDesc,
            ListSortDto::UpdatedAtAsc => Self::UpdatedAtAsc,
            ListSortDto::UpdatedAtDesc => Self::UpdatedAtDesc,
        }
    }
}

const MAX_SEARCH_CHARACTERS: usize = 100;

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListOntologiesQuery {
    #[param(minimum = 1, default = 1)]
    #[serde(default)]
    page: Option<u32>,
    #[param(minimum = 1, maximum = 100, default = 20)]
    #[serde(default)]
    per_page: Option<u16>,
    #[param(default = "-updated_at")]
    #[serde(default)]
    sort: Option<ListSortDto>,
    /// Optional case-insensitive substring matched against `name` and
    /// `display_name`; at most 100 characters.
    #[param(max_length = 100)]
    #[serde(default)]
    search: Option<String>,
}

impl TryFrom<ListOntologiesQuery> for ListOntologies {
    type Error = ApiError;

    fn try_from(value: ListOntologiesQuery) -> Result<Self, Self::Error> {
        let page = value.page.unwrap_or(1);
        let per_page = value.per_page.unwrap_or(20);
        if page == 0 || !(1..=100).contains(&per_page) {
            return Err(ApiError::new(ErrorKind::InvalidRequest));
        }
        let search = value
            .search
            .as_deref()
            .map(str::trim)
            .filter(|search| !search.is_empty());
        if search.is_some_and(|search| search.chars().count() > MAX_SEARCH_CHARACTERS) {
            return Err(ApiError::new(ErrorKind::InvalidRequest));
        }
        Ok(Self {
            page,
            per_page,
            sort: value.sort.map_or(ListSort::UpdatedAtDesc, Into::into),
            search: search.map(ToOwned::to_owned),
        })
    }
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct NeighborhoodQuery {
    #[param(minimum = 0, maximum = 5, default = 1)]
    #[serde(default)]
    depth: Option<u8>,
}

impl TryFrom<NeighborhoodQuery> for u8 {
    type Error = ApiError;

    fn try_from(value: NeighborhoodQuery) -> Result<Self, Self::Error> {
        let depth = value.depth.unwrap_or(1);
        if depth > 5 {
            return Err(ApiError::new(ErrorKind::InvalidRequest));
        }
        Ok(depth)
    }
}

/// Builds the Ontology metadata routes over the process application state.
pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/ontologies",
            get(list_ontologies)
                .post(create_ontology.layer(DefaultBodyLimit::max(ONTOLOGY_BODY_LIMIT))),
        )
        .route(
            "/v1/ontologies/{ontology_id}",
            get(get_ontology)
                .put(replace_ontology.layer(DefaultBodyLimit::max(ONTOLOGY_BODY_LIMIT)))
                .delete(delete_ontology),
        )
        .route(
            "/v1/ontologies/{ontology_id}/object-types/{object_type_id}/neighborhood",
            get(get_neighborhood),
        )
}

/// Lists Ontology summaries with deterministic pagination.
#[utoipa::path(
    get,
    path = "/v1/ontologies",
    tag = "Ontology",
    params(ListOntologiesQuery),
    responses(
        (status = 200, description = "one Ontology summary page", body = OntologyListResponse),
        (status = 400, description = "pagination, sort, or search query is invalid", body = ErrorResponse),
        (status = 500, description = "Ontology metadata could not be read", body = ErrorResponse),
        (status = 503, description = "Ontology storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn list_ontologies(
    State(state): State<Arc<AppState>>,
    query: Result<Query<ListOntologiesQuery>, QueryRejection>,
) -> Result<Json<OntologyListResponse>, ApiError> {
    record_ontology_operation("list");
    let Query(query) = query.map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let page = state.ontology().list(query.try_into()?).await?;
    record_ontology_success();
    Ok(Json(page.into()))
}

/// Creates an empty Ontology aggregate.
#[utoipa::path(
    post,
    path = "/v1/ontologies",
    tag = "Ontology",
    request_body(
        content = CreateOntologyRequest,
        description = "Complete JSON request; maximum encoded body size is 2 MiB"
    ),
    responses(
        (status = 201, description = "empty Ontology created", body = OntologyDto,
            headers(
                ("Location" = String, description = "canonical URI of the created Ontology"),
                ("ETag" = String, description = "current strong Ontology entity tag")
            )
        ),
        (status = 400, description = "request body is invalid", body = ErrorResponse),
        (status = 409, description = "Ontology name is already in use", body = ErrorResponse),
        (status = 413, description = "Ontology request body is too large", body = ErrorResponse),
        (status = 422, description = "Ontology metadata violates schema rules", body = OntologyValidationErrorResponse),
        (status = 500, description = "Ontology metadata could not be stored", body = ErrorResponse),
        (status = 503, description = "Ontology storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn create_ontology(
    State(state): State<Arc<AppState>>,
    request: Result<Json<CreateOntologyRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    record_ontology_operation("create");
    let request = ontology_json_request(request)?;
    let record = state.ontology().create(request.into()).await?;
    record_ontology_id(record.ontology.id);
    record_ontology_success();
    Ok(created_response(record))
}

/// Reads one complete Ontology aggregate.
#[utoipa::path(
    get,
    path = "/v1/ontologies/{ontology_id}",
    tag = "Ontology",
    params(("ontology_id" = OntologyIdDto, Path, description = "Ontology UUIDv7 identity")),
    responses(
        (status = 200, description = "complete current Ontology", body = OntologyDto,
            headers(("ETag" = String, description = "current strong Ontology entity tag"))),
        (status = 400, description = "path parameter is invalid", body = ErrorResponse),
        (status = 404, description = "Ontology was not found", body = ErrorResponse),
        (status = 500, description = "Ontology metadata could not be read", body = ErrorResponse),
        (status = 503, description = "Ontology storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_ontology(
    State(state): State<Arc<AppState>>,
    path: Result<Path<OntologyIdDto>, PathRejection>,
) -> Result<Response, ApiError> {
    record_ontology_operation("get");
    let Path(id) = path.map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let id = id
        .try_into()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    record_ontology_id(id);
    let record = state
        .ontology()
        .get(id)
        .await?
        .ok_or(OntologyStoreError::NotFound)?;
    record_ontology_success();
    Ok(record_response(record))
}

/// Replaces one complete Ontology aggregate when its strong ETag is current.
#[utoipa::path(
    put,
    path = "/v1/ontologies/{ontology_id}",
    tag = "Ontology",
    params(
        ("ontology_id" = OntologyIdDto, Path, description = "Ontology UUIDv7 identity"),
        ("If-Match" = String, Header, description = "one required current strong Ontology entity tag")
    ),
    request_body(
        content = OntologyDto,
        description = "Complete Ontology JSON document; maximum encoded body size is 2 MiB"
    ),
    responses(
        (status = 204, description = "Ontology replaced", body = (), headers(("ETag" = String, description = "new strong Ontology entity tag"))),
        (status = 400, description = "path, body, or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Ontology was not found", body = ErrorResponse),
        (status = 409, description = "Ontology name or child identity conflicts", body = ErrorResponse),
        (status = 412, description = "Ontology entity tag is stale", body = ErrorResponse),
        (status = 413, description = "Ontology request body is too large", body = ErrorResponse),
        (status = 422, description = "Ontology metadata violates schema rules", body = OntologyValidationErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Ontology metadata could not be stored", body = ErrorResponse),
        (status = 503, description = "Ontology storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn replace_ontology(
    State(state): State<Arc<AppState>>,
    path: Result<Path<OntologyIdDto>, PathRejection>,
    headers: HeaderMap,
    request: Result<Json<OntologyDto>, JsonRejection>,
) -> Result<Response, ApiError> {
    record_ontology_operation("replace");
    let Path(path_id) = path.map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let path_id = path_id
        .try_into()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    record_ontology_id(path_id);
    let expected_revision = expected_revision(&headers, path_id)?;
    let candidate: Ontology = ontology_json_request(request)?.try_into()?;
    if candidate.id != path_id {
        return Err(ApiError::new(ErrorKind::InvalidRequest));
    }
    let revision = state
        .ontology()
        .replace(&candidate, expected_revision)
        .await?;
    record_ontology_success();
    Ok(no_content_with_etag(path_id, revision))
}

/// Permanently deletes one Ontology aggregate when its strong ETag is current.
#[utoipa::path(
    delete,
    path = "/v1/ontologies/{ontology_id}",
    tag = "Ontology",
    params(
        ("ontology_id" = OntologyIdDto, Path, description = "Ontology UUIDv7 identity"),
        ("If-Match" = String, Header, description = "one required current strong Ontology entity tag")
    ),
    responses(
        (status = 204, description = "Ontology permanently deleted", body = ()),
        (status = 400, description = "path or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Ontology was not found", body = ErrorResponse),
        (status = 412, description = "Ontology entity tag is stale", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Ontology metadata could not be deleted", body = ErrorResponse),
        (status = 503, description = "Ontology storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn delete_ontology(
    State(state): State<Arc<AppState>>,
    path: Result<Path<OntologyIdDto>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    record_ontology_operation("delete");
    let Path(id) = path.map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let id = id
        .try_into()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    record_ontology_id(id);
    let expected_revision = expected_revision(&headers, id)?;
    state.ontology().delete(id, expected_revision).await?;
    record_ontology_success();
    Ok(StatusCode::NO_CONTENT)
}

/// Reads a persisted bidirectional Object Type neighborhood.
#[utoipa::path(
    get,
    path = "/v1/ontologies/{ontology_id}/object-types/{object_type_id}/neighborhood",
    tag = "Ontology",
    params(
        ("ontology_id" = OntologyIdDto, Path, description = "Ontology UUIDv7 identity"),
        ("object_type_id" = ObjectTypeIdDto, Path, description = "origin Object Type UUIDv7 identity"),
        NeighborhoodQuery,
    ),
    responses(
        (status = 200, description = "persisted induced Ontology subgraph", body = NeighborhoodDto),
        (status = 400, description = "path or depth is invalid", body = ErrorResponse),
        (status = 404, description = "Ontology or origin Object Type was not found", body = ErrorResponse),
        (status = 500, description = "Ontology metadata could not be read", body = ErrorResponse),
        (status = 503, description = "Ontology storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_neighborhood(
    State(state): State<Arc<AppState>>,
    path: Result<Path<(OntologyIdDto, ObjectTypeIdDto)>, PathRejection>,
    query: Result<Query<NeighborhoodQuery>, QueryRejection>,
) -> Result<Json<NeighborhoodDto>, ApiError> {
    record_ontology_operation("neighborhood");
    let Path((ontology_id, object_type_id)) =
        path.map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let ontology_id = ontology_id
        .try_into()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let object_type_id = object_type_id
        .try_into()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let Query(query) = query.map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let depth = query.try_into()?;
    record_ontology_id(ontology_id);
    Span::current().record("object_type_id", field::display(object_type_id));
    Span::current().record("ontology_depth", depth);
    let neighborhood = state
        .ontology()
        .neighborhood(ontology_id, object_type_id, depth)
        .await?;
    record_ontology_success();
    Ok(Json(neighborhood.into()))
}

fn deserialize_description<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .ok_or_else(|| serde::de::Error::custom("description must be omitted instead of null"))
        .map(Some)
}

fn ontology_json_request<T>(request: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    request.map(|Json(value)| value).map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::new(ErrorKind::OntologyPayloadTooLarge)
        } else {
            ApiError::new(ErrorKind::InvalidRequest)
        }
    })
}

fn created_response(record: OntologyRecord) -> Response {
    let id = record.ontology.id;
    let revision = record.revision;
    let mut response = (
        StatusCode::CREATED,
        Json(OntologyDto::from(record.ontology)),
    )
        .into_response();
    response.headers_mut().insert(
        LOCATION,
        // INVARIANT: a UUIDv7 Ontology ID yields a valid relative HTTP header value.
        HeaderValue::from_str(&format!("/v1/ontologies/{id}"))
            .expect("Ontology IDs always produce valid location headers"),
    );
    response
        .headers_mut()
        .insert(ETAG, etag_header(id, revision));
    response
}

fn record_response(record: OntologyRecord) -> Response {
    let id = record.ontology.id;
    let revision = record.revision;
    let mut response = Json(OntologyDto::from(record.ontology)).into_response();
    response
        .headers_mut()
        .insert(ETAG, etag_header(id, revision));
    response
}

fn no_content_with_etag(id: OntologyId, revision: i64) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(ETAG, etag_header(id, revision));
    response
}

fn etag_header(id: OntologyId, revision: i64) -> HeaderValue {
    // INVARIANT: canonical ETags contain only quoted entity-tag characters.
    HeaderValue::from_str(&canonical_etag(id, revision))
        .expect("canonical Ontology ETags always produce valid header values")
}

fn canonical_etag(id: OntologyId, revision: i64) -> String {
    format!("\"ontology:{id}:{revision}\"")
}

fn expected_revision(headers: &HeaderMap, id: OntologyId) -> Result<i64, ApiError> {
    let mut headers = headers.get_all(IF_MATCH).iter();
    let header = headers
        .next()
        .ok_or_else(|| ApiError::new(ErrorKind::OntologyPreconditionRequired))?;
    if headers.next().is_some() {
        return Err(ApiError::new(ErrorKind::InvalidRequest));
    }
    let tag = parse_strong_entity_tag(header)?;
    Ok(parse_canonical_etag(tag, id).unwrap_or(NEVER_MATCHING_REVISION))
}

fn parse_strong_entity_tag(header: &HeaderValue) -> Result<&[u8], ApiError> {
    let value = trim_optional_whitespace(header.as_bytes());
    if value == b"*" || value.starts_with(b"W/") {
        return Err(ApiError::new(ErrorKind::InvalidRequest));
    }
    let Some(tag) = value
        .strip_prefix(b"\"")
        .and_then(|tag| tag.strip_suffix(b"\""))
    else {
        return Err(ApiError::new(ErrorKind::InvalidRequest));
    };
    if !tag
        .iter()
        .all(|byte| *byte == b'!' || (b'#'..=b'~').contains(byte) || *byte >= 0x80)
    {
        return Err(ApiError::new(ErrorKind::InvalidRequest));
    }
    Ok(tag)
}

fn trim_optional_whitespace(mut value: &[u8]) -> &[u8] {
    while let Some((b' ' | b'\t', rest)) = value.split_first() {
        value = rest;
    }
    while let Some((b' ' | b'\t', rest)) = value.split_last() {
        value = rest;
    }
    value
}

fn parse_canonical_etag(tag: &[u8], id: OntologyId) -> Option<i64> {
    let tag = std::str::from_utf8(tag).ok()?;
    let mut segments = tag.split(':');
    let (Some("ontology"), Some(tagged_id), Some(revision), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return None;
    };
    let tagged_id = tagged_id.parse::<OntologyId>().ok()?;
    let revision = revision.parse::<i64>().ok()?;
    (tagged_id == id && revision > 0 && tag == format!("ontology:{tagged_id}:{revision}"))
        .then_some(revision)
}

fn record_ontology_id(id: OntologyId) {
    Span::current().record("ontology_id", field::display(id));
}

fn record_ontology_operation(operation: &'static str) {
    Span::current().record("operation", operation);
}

fn record_ontology_success() {
    Span::current().record("outcome", "success");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use utoipa::PartialSchema;

    const UUID_V7_PATTERN: &str =
        "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-7[0-9a-fA-F]{3}-[89aAbB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$";

    #[test]
    fn if_match_accepts_the_complete_strong_entity_tag_byte_grammar() {
        let header = HeaderValue::from_bytes(b" \t\"!#~\x80\xff\"\t ")
            .expect("RFC entity-tag bytes form a valid header value");

        let tag = parse_strong_entity_tag(&header).expect("strong entity-tag is accepted");

        assert_eq!(tag, b"!#~\x80\xff");
    }

    #[test]
    fn if_match_rejects_weak_wildcard_list_and_malformed_values() {
        for value in [
            "W/\"weak\"",
            "*",
            "\"first\", \"second\"",
            "not-quoted",
            "\"contains space\"",
            "\"contains\\\"quote\"",
        ] {
            let header = HeaderValue::from_str(value).expect("test value is a valid header value");

            assert!(
                matches!(
                    parse_strong_entity_tag(&header),
                    Err(error) if error.kind() == ErrorKind::InvalidRequest
                ),
                "{value} must not be accepted as one strong entity-tag"
            );
        }
    }

    #[test]
    fn valid_noncanonical_obs_text_if_match_becomes_a_stale_revision() {
        let id = OntologyId::new();
        let mut headers = HeaderMap::new();
        headers.insert(
            IF_MATCH,
            HeaderValue::from_bytes(b"\"opaque-\x80-tag\"")
                .expect("obs-text entity-tag is a valid header value"),
        );

        let revision = expected_revision(&headers, id).expect("strong entity-tag is well formed");

        assert_eq!(revision, NEVER_MATCHING_REVISION);
    }

    #[test]
    fn canonical_if_match_recovers_only_the_current_resource_revision() {
        let id = OntologyId::new();
        let header = HeaderValue::from_str(&canonical_etag(id, 42))
            .expect("canonical entity-tag is a valid header value");

        let tag = parse_strong_entity_tag(&header).expect("canonical entity-tag is strong");

        assert_eq!(parse_canonical_etag(tag, id), Some(42));
        assert_eq!(
            parse_canonical_etag(tag, OntologyId::new()),
            None,
            "a canonical tag for another resource is stale"
        );
    }

    #[test]
    fn list_query_trims_and_maps_a_valid_search_term() {
        let query = ListOntologiesQuery {
            page: None,
            per_page: None,
            sort: None,
            search: Some("  support  ".to_owned()),
        };

        let request = ListOntologies::try_from(query).expect("valid search is accepted");

        assert_eq!(request.search, Some("support".to_owned()));
        assert_eq!(request.page, 1);
        assert_eq!(request.per_page, 20);
        assert_eq!(request.sort, ListSort::UpdatedAtDesc);
    }

    #[test]
    fn list_query_treats_a_blank_search_term_as_absent() {
        for search in ["", "   "] {
            let query = ListOntologiesQuery {
                page: None,
                per_page: None,
                sort: None,
                search: Some(search.to_owned()),
            };

            let request = ListOntologies::try_from(query).expect("blank search is accepted");

            assert_eq!(request.search, None, "{search:?} must disable filtering");
        }
    }

    #[test]
    fn list_query_rejects_an_overlong_search_term() {
        let query = ListOntologiesQuery {
            page: None,
            per_page: None,
            sort: None,
            search: Some("x".repeat(MAX_SEARCH_CHARACTERS + 1)),
        };

        assert!(
            matches!(
                ListOntologies::try_from(query),
                Err(error) if error.kind() == ErrorKind::InvalidRequest
            ),
            "an overlong search term must be rejected"
        );
    }

    #[test]
    fn typed_id_openapi_schemas_constrain_uuid_v7() {
        assert_uuid_v7_schema::<OntologyIdDto>();
        assert_uuid_v7_schema::<ObjectTypeIdDto>();
        assert_uuid_v7_schema::<PropertyIdDto>();
        assert_uuid_v7_schema::<LinkTypeIdDto>();
    }

    fn assert_uuid_v7_schema<T: PartialSchema>() {
        let schema = serde_json::to_value(T::schema()).expect("schema serializes");

        assert_eq!(schema["type"], Value::String("string".to_owned()));
        assert_eq!(schema["format"], Value::String("uuid".to_owned()));
        assert_eq!(schema["pattern"], Value::String(UUID_V7_PATTERN.to_owned()));
    }
}
