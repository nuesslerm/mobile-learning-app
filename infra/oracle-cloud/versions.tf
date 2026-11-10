terraform {
  # 1.6 is the floor for the s3 backend's `endpoints` block (the older top-level
  # `endpoint` argument it replaces is deprecated and removed in 1.9+).
  required_version = ">= 1.6.0"

  required_providers {
    oci = {
      source  = "oracle/oci"
      version = ">= 5.0.0"
    }
  }

  # Remote state in OCI Object Storage, via its S3-compatible API.
  #
  # Terraform has no native OCI backend, so `s3` pointed at OCI's compat
  # endpoint is the supported route. It exists because state used to live ONLY
  # on whichever machine last ran `apply` (gitignored, no remote): a fresh clone
  # or a sandboxed agent session could not plan without re-importing every
  # resource by hand, and a lost laptop meant lost state. That is not
  # theoretical -- it is exactly why a 2026-08-11 credential rotation had to
  # recreate both VMs instead of running a two-line targeted apply.
  #
  # The bucket has OBJECT VERSIONING ENABLED, so every previous state file is
  # retained and recoverable -- the same "keep the old value" property whose
  # absence in Bitwarden Secrets Manager caused that incident.
  #
  # Encryption: objects are encrypted at rest with Oracle-managed AES-256 by
  # default; `encrypt = true` additionally asserts server-side encryption on
  # write. Access is NoPublicAccess and gated on the Customer Secret Key below.
  #
  # Credentials are the two halves of an OCI Customer Secret Key, kept in the
  # vault and exported as AWS_* for the terraform process only. ALWAYS invoke
  # terraform through `wf vm terraform ...`, which injects
  # them plus the two checksum env vars OCI's compat layer requires -- a bare
  # `terraform apply` authenticates fine and then fails 501 on the state WRITE.
  # For anything but a guarded apply (plan, init, import, workspace):
  # `wf vm terraform exec <dir> -- <args>`.
  # See clis/workflow-cli/src/data/infra/terraformBackendEnv.ts for both traps.
  #
  # LOCKING: OCI's S3 compat layer provides neither DynamoDB nor the
  # conditional-write primitive `use_lockfile` relies on, so there is NO state
  # locking here. Two concurrent applies can corrupt state. Coordinate manually
  # until that changes; versioning makes such a corruption recoverable rather
  # than fatal, which is the point.
  backend "s3" {
    bucket = "mobile-learning-tfstate"
    key    = "oracle-cloud/terraform.tfstate"
    region = "eu-frankfurt-1"

    endpoints = {
      s3 = "https://frbwmn2jazha.compat.objectstorage.eu-frankfurt-1.oraclecloud.com"
    }

    encrypt        = true
    use_path_style = true

    # OCI is not AWS: skip every AWS-specific preflight and the AWS-only
    # checksum header, all of which fail against the compat endpoint.
    skip_region_validation      = true
    skip_credentials_validation = true
    skip_requesting_account_id  = true
    skip_metadata_api_check     = true
    skip_s3_checksum            = true
  }
}

# Auth comes from terraform.tfvars (see terraform.tfvars.example) or, if you
# prefer, standard OCI env vars / ~/.oci/config. The explicit-vars form below is
# the most portable for CI.
provider "oci" {
  tenancy_ocid         = var.tenancy_ocid
  user_ocid            = var.user_ocid
  fingerprint          = var.api_key_fingerprint
  private_key_path     = var.api_private_key_path
  private_key_password = var.api_private_key_passphrase
  region               = var.region
}
