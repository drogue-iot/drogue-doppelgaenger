CREATE TABLE things (
    -- immutable data
    NAME VARCHAR(256) NOT NULL,
    APPLICATION VARCHAR(64) NOT NULL,
    UID uuid NOT NULL,
    CREATION_TIMESTAMP TIMESTAMP WITH TIME ZONE NOT NULL,

    -- resource information
    RESOURCE_VERSION uuid NOT NULL,
    GENERATION BIGINT NOT NULL,

    -- public metadata
    ANNOTATIONS JSON,
    LABELS JSONB, -- use JSONB as we index this column

    -- data
    DATA JSON,

    -- constraints
    PRIMARY KEY (NAME, APPLICATION)
);

CREATE INDEX things_labels ON things USING gin (labels);
