-- UI-only plan fixture. This is not evidence of a real model response.
BEGIN;
INSERT INTO projects(id,title,intent,state) VALUES
 ('51000000-0000-0000-0000-000000000001','网页验收：计划与编码','隔离界面验收数据，不代表真实模型执行','active');
INSERT INTO artifacts(id,project_id,title,kind,storage_path,media_type,sha256)
VALUES ('51000000-0000-0000-0000-000000000004','51000000-0000-0000-0000-000000000001',
 'fixture-plan.json','ai_file','ui-fixture/plan.json','text/plain',repeat('0',64));
INSERT INTO maitu_tasks(id,project_id,title,instruction,output_filename,task_kind,status)
VALUES ('51000000-0000-0000-0000-000000000002','51000000-0000-0000-0000-000000000001',
 '界面测试计划','仅用于计划编辑与采用测试','fixture-plan.json','plan','produced');
INSERT INTO maitu_attempts(id,task_id,number,status,input_snapshot,artifact_id,completed_at)
VALUES ('51000000-0000-0000-0000-000000000003','51000000-0000-0000-0000-000000000002',1,
 'produced','{"sources":[]}'::jsonb,'51000000-0000-0000-0000-000000000004',now());
UPDATE maitu_tasks SET latest_attempt_id='51000000-0000-0000-0000-000000000003' WHERE id='51000000-0000-0000-0000-000000000002';
INSERT INTO maitu_plans(attempt_id,project_id,proposal) VALUES
 ('51000000-0000-0000-0000-000000000003','51000000-0000-0000-0000-000000000001',
 '{"summary":"UI fixture: independently prepare then integrate","questions":[],"tasks":[
 {"key":"a","title":"计划任务 A","instruction":"整理 A","kind":"file","outputFilename":"a.md","acceptanceCriteria":"内容准确","dependsOn":[]},
 {"key":"b","title":"计划任务 B","instruction":"整理 B","kind":"file","outputFilename":"b.md","acceptanceCriteria":"内容准确","dependsOn":[]},
 {"key":"d","title":"计划整合","instruction":"固定引用 A B","kind":"file","outputFilename":"d.md","acceptanceCriteria":"包含两份来源","dependsOn":["a","b"]}
 ]}'::jsonb);
COMMIT;
