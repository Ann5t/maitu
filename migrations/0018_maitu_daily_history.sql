-- Daily-use history: cancelled is a real outcome with its own record; adopted
-- outputs carry the user's reason; a running attempt can request cancellation
-- before its next provider call.
ALTER TABLE maitu_tasks DROP CONSTRAINT IF EXISTS maitu_tasks_status_check;
ALTER TABLE maitu_tasks
  ADD CONSTRAINT maitu_tasks_status_check
  CHECK (status IN ('draft','queued','running','produced','failed','interrupted','cancelled'));

ALTER TABLE maitu_attempts DROP CONSTRAINT IF EXISTS maitu_attempts_status_check;
ALTER TABLE maitu_attempts
  ADD CONSTRAINT maitu_attempts_status_check
  CHECK (status IN ('queued','running','produced','failed','interrupted','cancelled'));

ALTER TABLE maitu_tasks ADD COLUMN IF NOT EXISTS accept_note text NOT NULL DEFAULT '';
ALTER TABLE maitu_attempts ADD COLUMN IF NOT EXISTS cancel_requested boolean NOT NULL DEFAULT false;
