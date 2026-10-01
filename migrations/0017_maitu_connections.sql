-- Multiple model connections: tasks may pin a connection key (empty = automatic
-- scheduling across usable connections). Historical attempts keep the key of the
-- connection that actually served them; the credential never enters the database.
ALTER TABLE maitu_tasks ADD COLUMN IF NOT EXISTS connection_key text NOT NULL DEFAULT '';
ALTER TABLE maitu_attempts ADD COLUMN IF NOT EXISTS connection_key text NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS maitu_attempts_running_connection
  ON maitu_attempts(connection_key) WHERE status = 'running';
