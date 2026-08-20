CREATE DATABASE stratum_studio_test;
CREATE DATABASE stratum_studio_corrupt_test;
CREATE DATABASE stratum_studio_postcommit_test;

\connect stratum_studio_corrupt_test

-- The corrupt database is reserved for one fail-closed startup test. SQLx
-- creates the Studio tables during `StudioStore::connect`; this event trigger
-- inserts a Provider without its required credential as soon as both tables
-- exist. The regular Studio test database remains pristine.
CREATE FUNCTION inject_missing_studio_credential()
RETURNS event_trigger
LANGUAGE plpgsql
AS $$
DECLARE
    provider_missing BOOLEAN;
BEGIN
    IF to_regclass('public.studio_catalog') IS NOT NULL
        AND to_regclass('public.studio_providers') IS NOT NULL
        AND to_regclass('public.studio_provider_credentials') IS NOT NULL
    THEN
        EXECUTE 'SELECT NOT EXISTS (
            SELECT 1 FROM studio_providers WHERE kind = ''deepseek''
        )' INTO provider_missing;

        IF provider_missing THEN
            EXECUTE 'INSERT INTO studio_catalog (singleton, revision)
                VALUES (TRUE, gen_random_uuid())
                ON CONFLICT (singleton) DO NOTHING';

            EXECUTE 'INSERT INTO studio_providers (kind, revision)
                VALUES (''deepseek'', gen_random_uuid())';
        END IF;
    END IF;
END;
$$;

CREATE EVENT TRIGGER inject_missing_studio_credential_after_ddl
ON ddl_command_end
WHEN TAG IN ('CREATE TABLE')
EXECUTE FUNCTION inject_missing_studio_credential();

\connect stratum_studio_postcommit_test

-- This database injects a valid Provider while migrations run, then removes
-- its credential in the same transaction as the first Model insert. It proves
-- the HTTP response for a committed Model create does not perform a second,
-- fallible Provider-catalog read after the durable write.
CREATE FUNCTION install_postcommit_model_fault()
RETURNS event_trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF to_regclass('public.studio_catalog') IS NOT NULL
        AND to_regclass('public.studio_providers') IS NOT NULL
        AND to_regclass('public.studio_provider_credentials') IS NOT NULL
        AND to_regclass('public.studio_models') IS NOT NULL
    THEN
        INSERT INTO studio_catalog (singleton, revision)
        VALUES (TRUE, gen_random_uuid())
        ON CONFLICT (singleton) DO NOTHING;

        INSERT INTO studio_providers (kind, revision)
        VALUES ('openai', gen_random_uuid())
        ON CONFLICT (kind) DO NOTHING;

        INSERT INTO studio_provider_credentials (provider_kind, secret)
        VALUES ('openai', 'postcommit-test-key')
        ON CONFLICT (provider_kind) DO NOTHING;

        IF NOT EXISTS (
            SELECT 1 FROM pg_trigger WHERE tgname = 'remove_credential_after_model_insert'
        ) THEN
            EXECUTE 'CREATE FUNCTION remove_credential_after_model_insert()
                RETURNS trigger LANGUAGE plpgsql AS $trigger$
                BEGIN
                    DELETE FROM studio_provider_credentials
                    WHERE provider_kind = NEW.provider_kind;
                    RETURN NEW;
                END;
                $trigger$';
            EXECUTE 'CREATE TRIGGER remove_credential_after_model_insert
                AFTER INSERT ON studio_models
                FOR EACH ROW EXECUTE FUNCTION remove_credential_after_model_insert()';
        END IF;
    END IF;
END;
$$;

CREATE EVENT TRIGGER install_postcommit_model_fault_after_ddl
ON ddl_command_end
WHEN TAG IN ('CREATE TABLE')
EXECUTE FUNCTION install_postcommit_model_fault();
