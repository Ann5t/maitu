-- An automatic retry must not be taken back by the connection that just throttled
-- it. The per-connection backoff lives in the worker process, so a claim that ran
-- in the same tick could still pick the retry before the backoff was consulted.
-- Record the hold on the task, inside the same transaction that re-queues it, and
-- let claim filter on it. An empty connection means no hold.
ALTER TABLE maitu_tasks ADD COLUMN IF NOT EXISTS hold_connection text NOT NULL DEFAULT '';
ALTER TABLE maitu_tasks ADD COLUMN IF NOT EXISTS hold_until timestamptz;