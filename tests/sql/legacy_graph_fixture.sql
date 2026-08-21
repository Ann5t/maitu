INSERT INTO projects (id, title, intent)
VALUES (
  '10000000-0000-0000-0000-000000000001',
  '旧模型迁移测试',
  '确认旧图记录在增量迁移后保持不变'
);

INSERT INTO project_branches (id, project_id, name, is_main)
VALUES (
  '10000000-0000-0000-0000-000000000002',
  '10000000-0000-0000-0000-000000000001',
  '旧主线', 1
);

INSERT INTO project_nodes
  (id, project_id, branch_id, kind, title, summary, actor_type)
VALUES (
  '10000000-0000-0000-0000-000000000003',
  '10000000-0000-0000-0000-000000000001',
  '10000000-0000-0000-0000-000000000002',
  'origin', '旧起点', '迁移前记录', 'human'
);
