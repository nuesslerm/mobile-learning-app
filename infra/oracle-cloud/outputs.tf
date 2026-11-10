output "vm_public_ip" {
  description = "Public IP of the single VM (every app: leetcode-backend, remote-chat-agent, leetcode-auth-cookie-capture, vm-gateway, vm-ops-api). Ephemeral: it changes on every instance recreate. Prefer the tailnet MagicDNS names for anything automated -- they are stable across recreates."
  value       = oci_core_instance.main.public_ip
}

# Ready-to-paste GitHub Actions secret value (the deploy workflows expect the
# `ubuntu@<ip>` form). See README for the `gh secret set` one-liner.
#
# The former vm_host_heavy output (and the VM_HOST_HEAVY secret it fed) is gone
# with the second VM -- all four deploy workflows now target VM_HOST. Delete
# VM_HOST_HEAVY from the repo secrets once this is applied, so a stale value
# can't quietly point a future workflow at a terminated host.
output "vm_host" {
  description = "Value for the VM_HOST GitHub secret."
  value       = "ubuntu@${oci_core_instance.main.public_ip}"
}

output "budget_ocid" {
  description = "OCID of the Terraform-managed cost budget (Cost Management -> Budgets in the console)."
  value       = oci_budget_budget.agent_cost_cap.id
}

output "compartment_id" {
  description = "Compartment OCID -- read by `wf vm terraform apply` (free-tier guard) via `terraform output -json`."
  value       = var.compartment_ocid
}

output "health_reports_ords_base_url" {
  description = "ORDS base URL for the health-reports Autonomous Database (see autonomous-db.tf). Auto-REST endpoints are served under <this>/<schema>/<object>/ once the one-time HEALTH_RPT schema setup has run."
  value       = oci_database_autonomous_database.health_reports.connection_urls[0].ords_url
}

output "health_reports_db_ocid" {
  description = "OCID of the health-reports Autonomous Database -- read by the one-time ORDS setup script to resolve a connection."
  value       = oci_database_autonomous_database.health_reports.id
}

output "health_reports_db_connection_string_low" {
  description = "The 'low' TNS connection string (host:port/service_name), for the one-time ORDS setup script's plain-TLS oracledb-thin-mode connection -- no wallet needed since is_mtls_connection_required = false. 'low' rather than 'high': this is a single ad-hoc admin script, not a workload needing dedicated parallel-query resources."
  value       = oci_database_autonomous_database.health_reports.connection_strings[0].all_connection_strings["LOW"]
  sensitive   = true
}
