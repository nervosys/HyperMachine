# Terraform Deployment

Infrastructure as Code for HyperMachine on AWS, Azure, and GCP.

## AWS EKS

```hcl
# main.tf
module "hypermachine" {
  source = "github.com/nervosys/terraform-hypermachine-aws"
  
  cluster_name     = "hypermachine-prod"
  region           = "us-west-2"
  node_count       = 3
  instance_type    = "m6i.xlarge"
  
  enable_gpu_nodes = true
  gpu_node_count   = 2
  gpu_instance_type = "g5.xlarge"
}

output "cluster_endpoint" {
  value = module.hypermachine.cluster_endpoint
}
```

## Azure AKS

```hcl
module "hypermachine" {
  source = "github.com/nervosys/terraform-hypermachine-azure"
  
  resource_group_name = "hypermachine-rg"
  location            = "westus2"
  cluster_name        = "hypermachine-prod"
  node_count          = 3
  vm_size             = "Standard_D4s_v3"
}
```

## GCP GKE

```hcl
module "hypermachine" {
  source = "github.com/nervosys/terraform-hypermachine-gcp"
  
  project_id   = "my-project"
  region       = "us-central1"
  cluster_name = "hypermachine-prod"
  node_count   = 3
  machine_type = "n2-standard-4"
}
```

## Usage

```bash
cd deploy/terraform
terraform init
terraform plan -var="environment=production"
terraform apply -var="environment=production"
```
