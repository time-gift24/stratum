INSERT INTO studio_catalog (singleton, revision)
VALUES (TRUE, gen_random_uuid())
ON CONFLICT (singleton) DO NOTHING;

ALTER TABLE studio_provider_credentials
    ADD CONSTRAINT studio_provider_credentials_secret_nonblank
    CHECK (length(btrim(secret)) > 0);
