CREATE TABLE IF NOT EXISTS idea_source_objects (
  id uuid PRIMARY KEY,
  idea_id uuid NOT NULL REFERENCES ideas(id) ON DELETE RESTRICT,
  client_request_id uuid NOT NULL,
  request_hash text NOT NULL CHECK (request_hash ~ '^sha256:[0-9a-f]{64}$'),
  kind text NOT NULL CHECK (kind IN ('file', 'image', 'audio')),
  original_filename text NOT NULL,
  display_name text NOT NULL,
  declared_media_type text,
  trusted_media_type text NOT NULL,
  size_bytes bigint NOT NULL CHECK (size_bytes > 0),
  sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
  storage_key text NOT NULL,
  note text NOT NULL DEFAULT '',
  created_by text NOT NULL CHECK (created_by IN ('human', 'agent', 'system')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (idea_id, client_request_id)
);

CREATE TABLE IF NOT EXISTS idea_revision_sources (
  idea_id uuid NOT NULL,
  idea_revision integer NOT NULL,
  source_id uuid NOT NULL REFERENCES idea_source_objects(id) ON DELETE RESTRICT,
  role text NOT NULL DEFAULT 'material' CHECK (role IN ('origin', 'material', 'evidence')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (idea_id, idea_revision, source_id),
  FOREIGN KEY (idea_id, idea_revision)
    REFERENCES idea_revisions(idea_id, revision) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idea_source_objects_idea_idx
  ON idea_source_objects (idea_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idea_source_objects_sha_idx
  ON idea_source_objects (sha256);

DO $$
DECLARE
  table_name text;
  trigger_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'idea_source_objects',
    'idea_revision_sources'
  ]
  LOOP
    trigger_name := table_name || '_immutable';
    IF NOT EXISTS (
      SELECT 1 FROM pg_trigger
      WHERE tgname = trigger_name AND tgrelid = table_name::regclass
    ) THEN
      EXECUTE format(
        'CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON %I '
        'FOR EACH ROW EXECUTE FUNCTION goal_reject_immutable_mutation()',
        trigger_name,
        table_name
      );
    END IF;
  END LOOP;
END;
$$;
