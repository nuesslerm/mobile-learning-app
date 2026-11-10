-- HEALTH_RPT.HOST_SAMPLES: the VM's metrics, one row per (timestamp, scope, name, metric), written and read by reporting-service as
-- health_rpt through ORDS's SQL endpoint (<ords root>/health/_/sql). The service creates the table itself on start (the statements
-- below, as `CREATE TABLE IF NOT EXISTS`), so this file is the reference, not a step to run. Nothing is rolled up: the series API
-- averages per minute or hour in SQL. Rows are deleted oldest first only when the database passes 90 % of the free tier's 20 GB
-- (reporting-service `prune`, every 6 h), followed by SHRINK SPACE so the segment's bytes drop (USER_SEGMENTS is what is measured).
CREATE TABLE HEALTH_RPT.HOST_SAMPLES (
  TS NUMBER(19) NOT NULL,
  SCOPE VARCHAR2(40) NOT NULL,
  NAME VARCHAR2(200) NOT NULL,
  METRIC VARCHAR2(80) NOT NULL,
  VALUE BINARY_DOUBLE NOT NULL,
  CONSTRAINT HOST_SAMPLES_PK PRIMARY KEY (TS, SCOPE, NAME, METRIC)
);
ALTER TABLE HEALTH_RPT.HOST_SAMPLES ENABLE ROW MOVEMENT;
