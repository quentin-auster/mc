ALTER TABLE artifacts DROP CONSTRAINT artifacts_object_key_key;
CREATE INDEX artifacts_object_key_idx ON artifacts(object_key);
