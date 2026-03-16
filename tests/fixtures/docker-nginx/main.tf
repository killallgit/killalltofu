terraform {
  required_providers {
    docker = {
      source  = "kreuzwerker/docker"
      version = "~> 3.0"
    }
  }
}

variable "docker_host" {
  description = "Docker daemon socket. Override for Colima, Podman, etc."
  type        = string
  default     = "unix:///var/run/docker.sock"
}

provider "docker" {
  host = var.docker_host
}

resource "docker_image" "nginx" {
  name         = "nginx:alpine"
  keep_locally = true
}

resource "docker_container" "nginx" {
  name  = "killalltofu-test"
  image = docker_image.nginx.image_id

  ports {
    internal = 80
    external = 8088
  }
}

output "container_name" {
  value = docker_container.nginx.name
}

output "url" {
  value = "http://localhost:8088"
}
