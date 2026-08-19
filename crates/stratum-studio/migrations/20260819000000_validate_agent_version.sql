ALTER TABLE studio_agent_definitions
    ADD CONSTRAINT studio_agent_definitions_version_tag_valid CHECK (
        octet_length(version) BETWEEN 1 AND 128
        AND version !~ '[[:cntrl:]]'
        AND version !~ '^[[:space:]]'
        AND version !~ '[[:space:]]$'
    );
