BEGIN;

INSERT INTO ideas (id, state, current_revision, created_by)
VALUES
  ('10000000-0000-0000-0000-000000000001', 'captured', 1, 'human'),
  ('10000000-0000-0000-0000-000000000002', 'captured', 1, 'human');
INSERT INTO idea_revisions
  (id, idea_id, revision, title, body, source_kind, created_by)
VALUES
  ('11000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000001', 1,
   '想法一', '这是第一个可追溯想法', 'text', 'human'),
  ('11000000-0000-0000-0000-000000000002', '10000000-0000-0000-0000-000000000002', 1,
   '想法二', '这是第二个可追溯想法', 'text', 'human');
SET CONSTRAINTS ALL IMMEDIATE;

DO $$
BEGIN
  BEGIN
    UPDATE idea_revisions SET body = '覆盖历史' WHERE idea_id =
      '10000000-0000-0000-0000-000000000001';
    RAISE EXCEPTION 'idea revision mutation unexpectedly succeeded';
  EXCEPTION WHEN SQLSTATE '55000' THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    INSERT INTO idea_links
      (id, source_idea_id, source_revision, target_idea_id, target_revision,
       relation, rationale, created_by)
    VALUES
      ('12000000-0000-0000-0000-000000000001',
       '10000000-0000-0000-0000-000000000001', 1,
       '10000000-0000-0000-0000-000000000001', 1,
       'related', '不应允许自关联', 'human');
    RAISE EXCEPTION 'self link unexpectedly succeeded';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

DO $$
BEGIN
  BEGIN
    INSERT INTO project_proposals
      (id, status, current_revision, approved_revision, created_by)
    VALUES
      ('13000000-0000-0000-0000-000000000001', 'approved', 1, 1, 'human');
    RAISE EXCEPTION 'approved proposal without project unexpectedly succeeded';
  EXCEPTION WHEN check_violation THEN
    NULL;
  END;
END;
$$;

ROLLBACK;
