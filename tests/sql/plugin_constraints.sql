BEGIN;

INSERT INTO plugin_publishers (publisher_id, display_name, public_key)
VALUES ('fudian.test', 'Fudian test publisher', repeat('1', 64));

DO $$
BEGIN
  BEGIN
    INSERT INTO plugin_publishers (publisher_id, display_name, public_key)
    VALUES ('fudian.bad', 'Bad key', 'not-a-key');
    RAISE EXCEPTION 'invalid publisher key was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

INSERT INTO plugin_packages (id, plugin_id, version, content_digest, manifest)
VALUES (
  '10000000-0000-0000-0000-000000000001',
  'fudian.tools.test',
  '1.0.0',
  'sha256:' || repeat('2', 64),
  '{}'::jsonb
);

INSERT INTO plugin_installations (
  id, plugin_package_id, publisher_id, signature, statement, statement_digest,
  runtime_image_digest, runtime_entry_digest, runner_digest, self_test, self_test_digest
)
VALUES (
  '10000000-0000-0000-0000-000000000002',
  '10000000-0000-0000-0000-000000000001',
  'fudian.test',
  repeat('3', 128),
  '{}'::jsonb,
  'sha256:' || repeat('4', 64),
  'sha256:' || repeat('5', 64),
  'sha256:' || repeat('6', 64),
  'sha256:' || repeat('7', 64),
  '{}'::jsonb,
  'sha256:' || repeat('8', 64)
);

INSERT INTO plugin_package_resources (
  id, plugin_package_id, path, media_type, content_digest, content
)
VALUES (
  '10000000-0000-0000-0000-000000000003',
  '10000000-0000-0000-0000-000000000001',
  'SKILL.md',
  'text/markdown; charset=utf-8',
  'sha256:' || repeat('9', 64),
  convert_to('# test skill', 'UTF8')
);

DO $$
BEGIN
  BEGIN
    INSERT INTO plugin_package_resources (
      id, plugin_package_id, path, media_type, content_digest, content
    ) VALUES (
      '10000000-0000-0000-0000-000000000004',
      '10000000-0000-0000-0000-000000000001',
      '../escape',
      'text/plain',
      'sha256:' || repeat('a', 64),
      convert_to('bad', 'UTF8')
    );
    RAISE EXCEPTION 'unsafe plugin resource path was accepted';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
  BEGIN
    UPDATE plugin_package_resources
    SET content = convert_to('changed', 'UTF8')
    WHERE id = '10000000-0000-0000-0000-000000000003';
    RAISE EXCEPTION 'plugin resource mutation was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    DELETE FROM plugin_package_resources
    WHERE id = '10000000-0000-0000-0000-000000000003';
    RAISE EXCEPTION 'plugin resource deletion was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    UPDATE plugin_installations
    SET signature = repeat('9', 128)
    WHERE id = '10000000-0000-0000-0000-000000000002';
    RAISE EXCEPTION 'installation proof mutation was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
  BEGIN
    DELETE FROM plugin_installations
    WHERE id = '10000000-0000-0000-0000-000000000002';
    RAISE EXCEPTION 'installation proof deletion was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

UPDATE plugin_installations
SET status = 'revoked', revoked_at = now(), revocation_reason = 'constraint test'
WHERE id = '10000000-0000-0000-0000-000000000002';

UPDATE plugin_publishers
SET status = 'revoked', revoked_at = now(), revocation_reason = 'constraint test'
WHERE publisher_id = 'fudian.test';

DO $$
BEGIN
  BEGIN
    UPDATE plugin_installations
    SET status = 'installed', revoked_at = NULL, revocation_reason = NULL
    WHERE id = '10000000-0000-0000-0000-000000000002';
    RAISE EXCEPTION 'revoked installation was reactivated';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
  BEGIN
    DELETE FROM plugin_publishers WHERE publisher_id = 'fudian.test';
    RAISE EXCEPTION 'publisher deletion was accepted';
  EXCEPTION WHEN object_not_in_prerequisite_state THEN
    NULL;
  END;
END;
$$;

ROLLBACK;
