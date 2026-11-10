-- HEALTH_RPT.WF_USAGE: one anonymised row per wf command (no host, no arguments, no key names). Run once as ADMIN through
-- REST-enabled SQL, one statement per request: POST <ords root>/admin/_/sql, Content-Type: application/sql. Rows land at
-- <ords root>/health/wf_usage/ (the WF_USAGE_ORDS_BASE_URL vault key), written as health_rpt.
CREATE TABLE HEALTH_RPT.WF_USAGE (
  ID NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  TS TIMESTAMP WITH TIME ZONE DEFAULT SYSTIMESTAMP NOT NULL,
  COMMAND VARCHAR2(200) NOT NULL,
  EXIT_CODE NUMBER NOT NULL,
  DURATION_MS NUMBER NOT NULL,
  ERROR VARCHAR2(1000),
  WF_VERSION VARCHAR2(64),
  OS VARCHAR2(32)
)
/
CREATE INDEX HEALTH_RPT.WF_USAGE_TS_IDX ON HEALTH_RPT.WF_USAGE (TS)
/
BEGIN
  ORDS.ENABLE_OBJECT(
    p_enabled => TRUE,
    p_schema => 'HEALTH_RPT',
    p_object => 'WF_USAGE',
    p_object_type => 'TABLE',
    p_object_alias => 'wf_usage',
    p_auto_rest_auth => TRUE
  );
  COMMIT;
END;
/
