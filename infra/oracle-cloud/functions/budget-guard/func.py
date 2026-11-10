"""
Budget guard — invoked by an OCI Events rule whenever a budget alert fires
(see ../../budget-guard.tf). Re-queries actual month-to-date spend itself
rather than trusting the triggering event's payload shape (which isn't
guaranteed to carry per-threshold detail) and decides the action from that
live number against the same two thresholds set in terraform.tfvars:
  - >= DESTROY_THRESHOLD_PCT of BUDGET_AMOUNT_USD -> terminate both VMs
  - >= STOP_THRESHOLD_PCT of BUDGET_AMOUNT_USD     -> stop both VMs (recoverable)
  - otherwise                                       -> no-op
"""
import io
import json
from datetime import datetime, timezone

import oci
from fdk import response

TERMINATED_STATES = ("TERMINATED", "TERMINATING")


def _month_to_date_spend(usage_client, tenancy_id, compartment_id):
    now = datetime.now(timezone.utc)
    start_of_month = now.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
    details = oci.usage_api.models.RequestSummarizedUsagesDetails(
        tenant_id=tenancy_id,
        time_usage_started=start_of_month,
        time_usage_ended=now,
        granularity="MONTHLY",
        query_type="COST",
        compartment_depth=6,
        filter=oci.usage_api.models.Filter(
            operator="AND",
            dimensions=[
                oci.usage_api.models.Dimension(key="compartmentId", value=compartment_id)
            ],
        ),
    )
    result = usage_client.request_summarized_usages(details)
    items = result.data.items or []
    return sum(item.computed_amount or 0 for item in items)


def handler(ctx, data: io.BytesIO = None):
    cfg = ctx.Config()
    tenancy_id = cfg["TENANCY_OCID"]
    compartment_id = cfg["COMPARTMENT_OCID"]
    budget_usd = float(cfg["BUDGET_AMOUNT_USD"])
    stop_pct = float(cfg.get("STOP_THRESHOLD_PCT", "50"))
    destroy_pct = float(cfg.get("DESTROY_THRESHOLD_PCT", "80"))
    instance_ids = [i for i in cfg["INSTANCE_IDS"].split(",") if i]

    signer = oci.auth.signers.get_resource_principals_signer()
    usage_client = oci.usage_api.UsageapiClient(config={}, signer=signer)
    compute_client = oci.core.ComputeClient(config={}, signer=signer)

    spend = _month_to_date_spend(usage_client, tenancy_id, compartment_id)
    pct = (spend / budget_usd * 100) if budget_usd > 0 else 0.0

    actions_taken = []
    if pct >= destroy_pct:
        for iid in instance_ids:
            try:
                state = compute_client.get_instance(iid).data.lifecycle_state
                if state not in TERMINATED_STATES:
                    compute_client.terminate_instance(iid, preserve_boot_volume=False)
                    actions_taken.append(f"terminated {iid} (was {state})")
            except Exception as e:  # noqa: BLE001 - report, don't crash the invocation
                actions_taken.append(f"ERROR terminating {iid}: {e}")
    elif pct >= stop_pct:
        for iid in instance_ids:
            try:
                state = compute_client.get_instance(iid).data.lifecycle_state
                if state == "RUNNING":
                    compute_client.instance_action(iid, "STOP")
                    actions_taken.append(f"stopped {iid}")
            except Exception as e:  # noqa: BLE001
                actions_taken.append(f"ERROR stopping {iid}: {e}")

    result = {
        "spend_usd": round(spend, 2),
        "budget_usd": budget_usd,
        "percent": round(pct, 1),
        "actions_taken": actions_taken,
    }
    print(f"budget-guard: {json.dumps(result)}", flush=True)
    return response.Response(
        ctx,
        response_data=json.dumps(result),
        headers={"Content-Type": "application/json"},
    )
