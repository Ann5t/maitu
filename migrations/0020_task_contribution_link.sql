-- goal_contributions ↔ maitu_tasks：把任务图的已采用成果接进目标枝干
-- （usage-boundaries 待修清单：两套记录此前在 schema 上无法互相指认，
--   "哪个任务产出这条贡献"只能写进正文文本）。只允许引用同项目且成果
--   已被采用的任务；attempt 取该任务 accepted_attempt_id 的快照，与
--   冻结候选同哲学——事后重试不改变已登记贡献指向的版本。
ALTER TABLE goal_contributions
  ADD COLUMN maitu_task_id uuid REFERENCES maitu_tasks(id) ON DELETE SET NULL,
  ADD COLUMN maitu_attempt_id uuid;

CREATE UNIQUE INDEX goal_contributions_maitu_attempt_key
  ON goal_contributions (maitu_attempt_id) WHERE maitu_attempt_id IS NOT NULL;
