-- HEALTH_RPT.POOL_COOLDOWNS: one row per model pool that hit a usage cap, shared by every caller of the model ladder (rca-ladder's
-- OrdsLedgerStore). ENTRY is the cooldown as JSON (coldUntil, learnedResetMs, lastCapAt, ...). Run once as ADMIN through REST-enabled
-- SQL, one statement per request: POST <ords root>/admin/_/sql, Content-Type: application/sql. Rows land at
-- <ords root>/health/pool_cooldowns/ (the POOL_COOLDOWNS_ORDS_BASE_URL vault key), read and written as health_rpt. A row is
-- replaced with PUT <url><pool>, so every column is sent: ORDS binds an omitted column to NULL, not to its default.
CREATE TABLE HEALTH_RPT.POOL_COOLDOWNS (
  POOL VARCHAR2(100) PRIMARY KEY,
  ENTRY VARCHAR2(2000) NOT NULL,
  UPDATED_AT TIMESTAMP WITH TIME ZONE DEFAULT SYSTIMESTAMP NOT NULL
)
/
BEGIN
  ORDS.ENABLE_OBJECT(
    p_enabled => TRUE,
    p_schema => 'HEALTH_RPT',
    p_object => 'POOL_COOLDOWNS',
    p_object_type => 'TABLE',
    p_object_alias => 'pool_cooldowns',
    p_auto_rest_auth => TRUE
  );
  COMMIT;
END;
/
