// PagerDuty Custom Event Transformer script for security-alerts.tf's alerts.
//
// NOT deployable via Terraform — PagerDuty only accepts this through its own
// UI code editor. Paste the whole file as the transformer's script body:
//   PagerDuty -> Services -> (the service security-alerts.tf pages)
//   -> Integrations tab -> the Custom Event Transformer integration
//   -> "Edit Transformer" (or similar) -> replace the default script with
//   this -> Save.
//
// Deployed at: https://mobile-learning.eu.pagerduty.com/services/PDZA7DG/integrations/P9N891K
// (service PDZA7DG, this integration P9N891K) — re-paste this file there if
// it's ever changed, since PagerDuty doesn't version or sync this script
// with the repo on its own.
//
// Why this exists: without a transform script, Custom Event Transformer
// falls back to a bare-bones default, which is why the first incidents
// looked like generic noise instead of "what happened, who did it, where."
//
// OCI's Events service delivers the raw event JSON directly as the POST
// body (metadata like message type lives in X-OCI-NS-* headers instead,
// not wrapped into the body the way e.g. AWS SNS wraps its payloads) — this
// parses that shape directly. Every OCI event uses this same envelope
// (eventType / eventTime / data.identity / data.resourceName / eventID)
// regardless of which service produced it, so this one script covers every
// event type security-alerts.tf watches for.
//
// Verify after pasting: re-run scripts/test-security-alert.sh and check
// the resulting incident's summary — it should read something like
// "com.oraclecloud.identityControlPlane.CreateAuthToken by agent-terraform
// in <compartment>", not a generic/empty description.
//
// CONFIRMED LIMITATION (verified against a real incident's full raw
// payload, not guessed): OCI's Events-delivered JSON for at least
// virtualnetwork.createnetworksecuritygroup has NO `data.identity` key at
// all -- not missing sub-fields, the whole object is absent. Unlike the
// Audit service's own query API (`oci audit event list`), which always
// includes identity, the real-time Events payload apparently doesn't for
// (at least) this event type. The one usable substitute is
// `data.definedTags['Oracle-Tags'].CreatedBy`, which OCI auto-populates on
// every created resource -- but it names the resource's ORIGINAL creator,
// not necessarily whoever triggered THIS specific event (wrong for a
// later update/delete by someone else), so it's labeled accordingly below
// rather than presented as certain. True per-action attribution for event
// types missing `identity` would need server-side enrichment (an OCI
// Function querying Audit, the same approach check-quota-drift.sh uses) --
// not built here since deploying a Function needs Docker, which this
// repo's sandbox doesn't have (see budget-guard.tf's same blocker).

var body = null;
try {
  body = JSON.parse(PD.inputRequest.rawBody);
} catch (_e) {
  body = null;
}

if (body && body.eventType) {
  var data = body.data || {};
  var identity = data.identity || {};
  var eventType = body.eventType;
  var eventTime = body.eventTime || new Date().toISOString();

  var principal = identity.principalName || identity.callerName;
  if (!principal) {
    var ociTags = data.definedTags && data.definedTags['Oracle-Tags'];
    principal = (ociTags && ociTags.CreatedBy)
      ? ociTags.CreatedBy + ' (resource creator -- OCI omitted an actor identity from this event type, so this is the original creator tag, not necessarily who triggered this specific event)'
      : 'unknown principal (OCI omitted identity from this event type entirely)';
  }
  var compartment = data.compartmentName || data.compartmentId || 'unknown compartment';
  var resource = data.resourceName || data.resourceId || '';
  var sourceIp = identity.ipAddress || 'unknown IP';

  var summary = eventType + ' by ' + principal + ' in ' + compartment + (resource ? ' (' + resource + ')' : '');

  // A fresh incident per occurrence (not deduped into one open incident) --
  // every individual security event here is worth its own review, unlike
  // an infra alert where repeated "still down" pings should collapse.
  var normalized_event = {
    incident_key: eventType + ':' + (body.eventID || eventTime),
    event_type: PD.Trigger,
    description: summary,
    details: {
      eventType: eventType,
      eventTime: eventTime,
      principal: principal,
      sourceIp: sourceIp,
      compartment: compartment,
      resource: resource,
      raw: body
    }
  };

  PD.emitGenericEvents([normalized_event]);
} else {
  // Not a recognizable OCI event payload (e.g. a stray ping, or the
  // subscription confirmation handshake, which PagerDuty's platform-level
  // OCI support already handles before this script runs) -- no incident.
  PD.emitGenericEvents([]);
}
