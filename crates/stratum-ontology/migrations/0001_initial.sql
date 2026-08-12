CREATE TABLE ontologies (
    id uuid NOT NULL,
    name text NOT NULL,
    display_name text NOT NULL,
    description text,
    revision bigint NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT ontologies_pkey PRIMARY KEY (id),
    CONSTRAINT ontologies_name_key UNIQUE (name),
    CONSTRAINT ontologies_revision_check CHECK (revision > 0)
);

CREATE TABLE ontology_object_types (
    id uuid NOT NULL,
    ontology_id uuid NOT NULL,
    name text NOT NULL,
    display_name text NOT NULL,
    description text,
    sort_order integer NOT NULL,
    CONSTRAINT ontology_object_types_pkey PRIMARY KEY (id),
    CONSTRAINT ontology_object_types_ontology_id_fkey
        FOREIGN KEY (ontology_id) REFERENCES ontologies (id) ON DELETE CASCADE,
    CONSTRAINT ontology_object_types_ontology_id_name_key UNIQUE (ontology_id, name),
    CONSTRAINT ontology_object_types_ontology_id_id_key UNIQUE (ontology_id, id),
    CONSTRAINT ontology_object_types_sort_order_check CHECK (sort_order >= 0)
);

CREATE TABLE ontology_properties (
    id uuid NOT NULL,
    object_type_id uuid NOT NULL,
    name text NOT NULL,
    display_name text NOT NULL,
    description text,
    value_type text NOT NULL,
    required boolean NOT NULL,
    sort_order integer NOT NULL,
    CONSTRAINT ontology_properties_pkey PRIMARY KEY (id),
    CONSTRAINT ontology_properties_object_type_id_fkey
        FOREIGN KEY (object_type_id)
        REFERENCES ontology_object_types (id) ON DELETE CASCADE,
    CONSTRAINT ontology_properties_object_type_id_name_key UNIQUE (object_type_id, name),
    CONSTRAINT ontology_properties_value_type_check
        CHECK (value_type IN ('string', 'integer', 'number', 'boolean', 'date', 'date_time')),
    CONSTRAINT ontology_properties_sort_order_check CHECK (sort_order >= 0)
);

CREATE TABLE ontology_link_types (
    id uuid NOT NULL,
    ontology_id uuid NOT NULL,
    name text NOT NULL,
    display_name text NOT NULL,
    description text,
    source_object_type_id uuid NOT NULL,
    target_object_type_id uuid NOT NULL,
    source_to_target text NOT NULL,
    target_to_source text NOT NULL,
    sort_order integer NOT NULL,
    CONSTRAINT ontology_link_types_pkey PRIMARY KEY (id),
    CONSTRAINT ontology_link_types_ontology_id_fkey
        FOREIGN KEY (ontology_id) REFERENCES ontologies (id) ON DELETE CASCADE,
    CONSTRAINT ontology_link_types_ontology_id_name_key UNIQUE (ontology_id, name),
    CONSTRAINT ontology_link_types_source_endpoint_fkey
        FOREIGN KEY (ontology_id, source_object_type_id)
        REFERENCES ontology_object_types (ontology_id, id),
    CONSTRAINT ontology_link_types_target_endpoint_fkey
        FOREIGN KEY (ontology_id, target_object_type_id)
        REFERENCES ontology_object_types (ontology_id, id),
    CONSTRAINT ontology_link_types_source_to_target_check
        CHECK (source_to_target IN ('one', 'many')),
    CONSTRAINT ontology_link_types_target_to_source_check
        CHECK (target_to_source IN ('one', 'many')),
    CONSTRAINT ontology_link_types_sort_order_check CHECK (sort_order >= 0)
);

CREATE TABLE ontology_canvas_positions (
    object_type_id uuid NOT NULL,
    x double precision NOT NULL,
    y double precision NOT NULL,
    sort_order integer NOT NULL,
    CONSTRAINT ontology_canvas_positions_pkey PRIMARY KEY (object_type_id),
    CONSTRAINT ontology_canvas_positions_object_type_id_fkey
        FOREIGN KEY (object_type_id)
        REFERENCES ontology_object_types (id) ON DELETE CASCADE,
    CONSTRAINT ontology_canvas_positions_x_finite_check
        CHECK (x > '-Infinity'::double precision AND x < 'Infinity'::double precision),
    CONSTRAINT ontology_canvas_positions_y_finite_check
        CHECK (y > '-Infinity'::double precision AND y < 'Infinity'::double precision),
    CONSTRAINT ontology_canvas_positions_sort_order_check CHECK (sort_order >= 0)
);

CREATE INDEX ontology_object_types_ontology_id_sort_order_id_idx
    ON ontology_object_types (ontology_id, sort_order, id);

CREATE INDEX ontology_properties_object_type_id_sort_order_id_idx
    ON ontology_properties (object_type_id, sort_order, id);

CREATE INDEX ontology_link_types_ontology_id_sort_order_id_idx
    ON ontology_link_types (ontology_id, sort_order, id);

CREATE INDEX ontology_link_types_ontology_id_source_object_type_id_idx
    ON ontology_link_types (ontology_id, source_object_type_id);

CREATE INDEX ontology_link_types_ontology_id_target_object_type_id_idx
    ON ontology_link_types (ontology_id, target_object_type_id);
