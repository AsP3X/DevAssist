//! What Docker reports, read from the JSON its commands print.

use serde_json::Value;

/// The label Compose puts on a container to say which project it is in.
const PROJECT_LABEL: &str = "com.docker.compose.project=";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    pub name: String,
    /// The image it was made from, without a digest.
    pub image: String,
    /// `running`, `exited`, `paused`, `created`, `restarting` or `dead`.
    pub state: String,
    /// Docker's own words, such as "Up 3 hours (healthy)".
    pub status: String,
    /// Published ports, as `8081→80`.
    pub ports: String,
    /// The Compose project it belongs to. Empty for none.
    pub project: String,
    /// Volume names and host paths it mounts.
    pub mounts: Vec<String>,
}

impl Container {
    /// True when it has to be stopped before it can be removed.
    pub fn running(&self) -> bool {
        matches!(self.state.as_str(), "running" | "paused" | "restarting")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    /// Compose's own words, such as "running(2), exited(1)".
    pub status: String,
    pub running: usize,
    pub total: usize,
    /// The compose files it was started from.
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    /// The whole ID, with its `sha256:` prefix.
    pub id: String,
    /// Empty for an image without a name.
    pub repository: String,
    pub tag: String,
    /// Docker's own words, such as "242MB".
    pub size: String,
    /// Docker's own words, such as "2 weeks ago".
    pub age: String,
    /// How many containers Docker says use it. Older versions do not say.
    pub containers: usize,
}

impl Image {
    /// True for an image no name points at any more.
    pub fn dangling(&self) -> bool {
        self.repository.is_empty()
    }

    /// `repository:tag`, or `<none>` for a dangling image.
    pub fn name(&self) -> String {
        match (self.repository.is_empty(), self.tag.is_empty()) {
            (true, _) => "<none>".to_string(),
            (false, true) => self.repository.clone(),
            (false, false) => format!("{}:{}", self.repository, self.tag),
        }
    }

    /// The first twelve characters of the ID, as `docker images` shows it.
    pub fn short_id(&self) -> String {
        short_id(&self.id)
    }

    /// What to hand to `docker rmi`: the name, or the ID when it has none.
    pub fn reference(&self) -> String {
        if self.dangling() || self.tag.is_empty() {
            self.id.clone()
        } else {
            self.name()
        }
    }

    /// One name per row: an image with two tags is listed twice.
    pub fn key(&self) -> String {
        format!("{}|{}", self.id, self.name())
    }

    /// The names of the containers made from this image.
    pub fn used_by(&self, containers: &[Container]) -> Vec<String> {
        let name = self.name();
        let short = self.short_id();
        containers
            .iter()
            .filter(|container| {
                let image = container.image.as_str();
                image == name
                    || (self.tag == "latest" && image == self.repository)
                    || (!image.is_empty() && (self.id.ends_with(image) || image == short))
            })
            .map(|container| container.name.clone())
            .collect()
    }

    pub fn in_use(&self, containers: &[Container]) -> bool {
        self.containers > 0 || !self.used_by(containers).is_empty()
    }

    /// The size in bytes, as near as Docker's rounded text allows.
    pub fn bytes(&self) -> u64 {
        parse_size(&self.size)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    pub driver: String,
}

impl Volume {
    /// True for a volume Docker named itself: 64 hex digits.
    pub fn anonymous(&self) -> bool {
        self.name.len() == 64 && self.name.chars().all(|ch| ch.is_ascii_hexdigit())
    }

    /// The name, with an anonymous one cut to twelve characters.
    pub fn label(&self) -> String {
        if self.anonymous() {
            self.name[..12].to_string()
        } else {
            self.name.clone()
        }
    }

    /// The names of the containers that mount this volume.
    pub fn used_by(&self, containers: &[Container]) -> Vec<String> {
        containers
            .iter()
            .filter(|container| container.mounts.contains(&self.name))
            .map(|container| container.name.clone())
            .collect()
    }
}

/// Everything the tool shows, as one load found it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lists {
    pub containers: Vec<Container>,
    pub projects: Vec<Project>,
    pub images: Vec<Image>,
    pub volumes: Vec<Volume>,
}

/// One JSON object per line, as `--format '{{json .}}'` prints. A line
/// that is not an object, such as a warning, is skipped.
fn objects(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter(Value::is_object)
        .collect()
}

/// A field as text. Missing fields are empty: Docker's versions differ.
fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// The lines of `docker ps -a --no-trunc --format '{{json .}}'`, sorted by
/// project and name.
pub fn parse_containers(output: &str) -> Vec<Container> {
    let mut containers: Vec<Container> = objects(output)
        .iter()
        .map(|value| {
            let image = text(value, "Image");
            Container {
                id: text(value, "ID"),
                name: text(value, "Names"),
                image: image.split('@').next().unwrap_or_default().to_string(),
                state: text(value, "State"),
                status: text(value, "Status"),
                ports: short_ports(&text(value, "Ports")),
                project: project_of(&text(value, "Labels")),
                mounts: text(value, "Mounts")
                    .split(',')
                    .map(str::trim)
                    .filter(|mount| !mount.is_empty())
                    .map(str::to_string)
                    .collect(),
            }
        })
        .filter(|container| !container.id.is_empty())
        .collect();
    containers.sort_by(|left, right| {
        (left.project.is_empty(), &left.project, &left.name).cmp(&(
            right.project.is_empty(),
            &right.project,
            &right.name,
        ))
    });
    containers
}

/// The Compose project named in a container's labels. The labels come as
/// one text, `key=value,key=value`, and a value may hold commas itself.
fn project_of(labels: &str) -> String {
    labels
        .split(',')
        .find_map(|label| label.strip_prefix(PROJECT_LABEL))
        .unwrap_or_default()
        .to_string()
}

/// "0.0.0.0:8081->80/tcp, [::]:8081->80/tcp" as "8081→80". A port that is
/// only exposed, not published, is left out.
pub fn short_ports(ports: &str) -> String {
    let mut seen: Vec<String> = Vec::new();
    for part in ports.split(',') {
        let Some((host, inner)) = part.trim().split_once("->") else {
            continue;
        };
        let host = host.rsplit(':').next().unwrap_or(host);
        let inner = inner.split('/').next().unwrap_or(inner);
        let short = format!("{host}→{inner}");
        if !seen.contains(&short) {
            seen.push(short);
        }
    }
    seen.join(" ")
}

/// The lines of `docker image ls --no-trunc --format '{{json .}}'`, sorted
/// by name with the nameless ones last.
pub fn parse_images(output: &str) -> Vec<Image> {
    let named = |field: String| {
        if field == "<none>" {
            String::new()
        } else {
            field
        }
    };
    let mut images: Vec<Image> = objects(output)
        .iter()
        .map(|value| Image {
            id: text(value, "ID"),
            repository: named(text(value, "Repository")),
            tag: named(text(value, "Tag")),
            size: text(value, "Size"),
            age: text(value, "CreatedSince"),
            containers: text(value, "Containers").parse().unwrap_or(0),
        })
        .filter(|image| !image.id.is_empty())
        .collect();
    images.sort_by(|left, right| {
        (left.dangling(), left.name(), &left.id).cmp(&(right.dangling(), right.name(), &right.id))
    });
    images
}

/// The lines of `docker volume ls --format '{{json .}}'`, sorted by name
/// with the anonymous ones last.
pub fn parse_volumes(output: &str) -> Vec<Volume> {
    let mut volumes: Vec<Volume> = objects(output)
        .iter()
        .map(|value| Volume {
            name: text(value, "Name"),
            driver: text(value, "Driver"),
        })
        .filter(|volume| !volume.name.is_empty())
        .collect();
    volumes.sort_by(|left, right| {
        (left.anonymous(), &left.name).cmp(&(right.anonymous(), &right.name))
    });
    volumes
}

/// The output of `docker compose ls -a --format json`: one JSON list.
pub fn parse_projects(output: &str) -> Vec<Project> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(output.trim()) else {
        return Vec::new();
    };
    let mut projects: Vec<Project> = items
        .iter()
        .map(|value| {
            let status = text(value, "Status");
            let (running, total) = count_status(&status);
            Project {
                name: text(value, "Name"),
                status,
                running,
                total,
                files: text(value, "ConfigFiles")
                    .split(',')
                    .map(str::trim)
                    .filter(|file| !file.is_empty())
                    .map(str::to_string)
                    .collect(),
            }
        })
        .filter(|project| !project.name.is_empty())
        .collect();
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    projects
}

/// "running(2), exited(1)" as (2, 3): how many run, and how many there are.
fn count_status(status: &str) -> (usize, usize) {
    let mut running = 0;
    let mut total = 0;
    for part in status.split(',') {
        let Some((state, count)) = part.trim().split_once('(') else {
            continue;
        };
        let count: usize = count.trim_end_matches(')').parse().unwrap_or(0);
        total += count;
        if state.trim() == "running" {
            running += count;
        }
    }
    (running, total)
}

/// Whether a line of `docker events --format '{{json .}}'` changes what
/// the lists show. Health checks run a command in the container every few
/// seconds, and that is not news.
pub fn event_matters(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        return false;
    };
    let action = match text(&value, "Action") {
        action if action.is_empty() => text(&value, "status"),
        action => action,
    };
    !(action.is_empty() || action.starts_with("exec_") || action.starts_with("health_status"))
}

/// The first twelve characters of an ID, without `sha256:`.
pub fn short_id(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

/// "242MB" as bytes. Docker counts in thousands.
pub fn parse_size(size: &str) -> u64 {
    let size = size.trim();
    let split = size
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .unwrap_or(size.len());
    let number: f64 = size[..split].parse().unwrap_or(0.0);
    let factor = match size[split..].trim().to_ascii_lowercase().as_str() {
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        _ => 1.0,
    };
    (number * factor) as u64
}

/// Bytes as Docker would word them: "1.2GB", "242MB".
pub fn format_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes >= 1e9 {
        format!("{:.1}GB", bytes / 1e9)
    } else if bytes >= 1e6 {
        format!("{:.0}MB", bytes / 1e6)
    } else if bytes >= 1e3 {
        format!("{:.0}kB", bytes / 1e3)
    } else {
        format!("{bytes:.0}B")
    }
}

#[cfg(test)]
pub mod samples {
    //! Lines as Docker 29.7 printed them, shortened, with secrets taken out.

    pub const PS: &str = r#"{"Command":"\"/docker-entrypoint.sh nginx -g 'daemon off;'\"","CreatedAt":"2026-11-12 19:28:50 +0100 CET","HealthStatus":"healthy","ID":"ce80c9f75bddd12cdac2e03742498643dc5a1a130b64235dfa5458b92672d323","Image":"shop-web","Labels":"com.docker.compose.config-hash=1505981f,com.docker.compose.depends_on=api:service_healthy:false,com.docker.compose.project.config_files=C:\\dev\\shop\\docker-compose.yml,C:\\dev\\shop\\docker-compose.local.yml,com.docker.compose.project.working_dir=C:\\dev\\shop,com.docker.compose.project=shop,com.docker.compose.service=web","LocalVolumes":"0","Mounts":"","Names":"shop-web","Networks":"shop-internal","Platform":{"architecture":"amd64","os":"linux"},"Ports":"0.0.0.0:8081-\u003e80/tcp, [::]:8081-\u003e80/tcp","RunningFor":"35 seconds ago","Size":"4.1kB (virtual 167MB)","State":"running","Status":"Up 28 seconds (healthy)"}
{"Command":"\"docker-entrypoint.sh postgres\"","CreatedAt":"2026-11-12 19:27:18 +0100 CET","HealthStatus":"healthy","ID":"67bf4d90761d9e83b668a6edbe0b8fcdddb52381e63f981639d1bcafe5b0c022","Image":"postgres:16-alpine@sha256:aa6cb33cf4c19c765cefcf48d96176258800870a9a1e930ad126200f9d825139","Labels":"com.docker.compose.project.config_files=C:\\dev\\shop\\docker-compose.yml,C:\\dev\\shop\\docker-compose.local.yml,com.docker.compose.project=shop,com.docker.compose.service=postgres","LocalVolumes":"1","Mounts":"/run/desktop/mnt/host/c/dev/shop/init.sh,shop_pg_data","Names":"shop-postgres","Networks":"shop-internal","Platform":{"architecture":"amd64","os":"linux"},"Ports":"127.0.0.1:15432-\u003e5432/tcp","RunningFor":"2 minutes ago","Size":"28.7kB (virtual 303MB)","State":"running","Status":"Up 2 minutes (healthy)"}
{"Command":"\"/shop-admin\"","CreatedAt":"2026-11-12 19:28:49 +0100 CET","HealthStatus":"none","ID":"0b8aebeb54cfdedd9ed677b93b67ab015e85bb3986614f35a7cbd52529a51f2b","Image":"shop-admin","Labels":"com.docker.compose.project=shop,com.docker.compose.service=admin","LocalVolumes":"0","Mounts":"","Names":"shop-admin","Networks":"shop-internal","Platform":{"architecture":"amd64","os":"linux"},"Ports":"8082/tcp","RunningFor":"36 seconds ago","Size":"12.3kB (virtual 38MB)","State":"exited","Status":"Exited (0) 2 days ago"}
{"Command":"\"/usr/bin/tini -- /usr/sbin/sshd -D -e\"","CreatedAt":"2026-09-11 08:34:37 +0200 CEST","HealthStatus":"none","ID":"16a86105f8bca7542ca22d0b74f46be6e6cb0d7c16651f0c50591dcd893559d2","Image":"backups-tools:latest","Labels":"desktop.docker.io/binds/0/Source=D:/00_Backups,desktop.docker.io/ports.scheme=v2","LocalVolumes":"1","Mounts":"/run/desktop/mnt/host/d/00_Backups,1b441f4d9016a6925b9b642a430c1fff191d8407ef4d1d881991214817a5e9f0","Names":"backups","Networks":"bridge","Platform":{"architecture":"amd64","os":"linux"},"Ports":"","RunningFor":"2 months ago","Size":"22.5MB (virtual 135MB)","State":"exited","Status":"Exited (0) 5 weeks ago"}"#;

    pub const IMAGES: &str = r#"{"Containers":"1","CreatedAt":"2026-11-12 19:28:47 +0100 CET","CreatedSince":"38 seconds ago","Digest":"\u003cnone\u003e","ID":"sha256:e12f7cf5da4f72cecde6b04af9c370628bec7aef05b687b49b0203cca18b886f","Repository":"shop-web","SharedSize":"N/A","Size":"242MB","Tag":"latest","UniqueSize":"N/A"}
{"Containers":"N/A","CreatedAt":"2026-10-01 10:00:00 +0200 CEST","CreatedSince":"6 weeks ago","Digest":"\u003cnone\u003e","ID":"sha256:ac838714b7d794185a866e5a6b479b954e7d8a070568aa0acac6fe07c3070bf5","Repository":"postgres","SharedSize":"N/A","Size":"394MB","Tag":"16-alpine","UniqueSize":"N/A"}
{"Containers":"0","CreatedAt":"2026-11-12 18:04:34 +0100 CET","CreatedSince":"About an hour ago","Digest":"\u003cnone\u003e","ID":"sha256:532d42f059e8419279745448d7b689f20986e6ea141efc830294824b63cb255a","Repository":"old-admin","SharedSize":"N/A","Size":"50.8MB","Tag":"latest","UniqueSize":"N/A"}
{"Containers":"0","CreatedAt":"2026-11-10 09:00:00 +0100 CET","CreatedSince":"2 days ago","Digest":"\u003cnone\u003e","ID":"sha256:8524458b60e33c7af75cac1361695de390c2f861c6095ef9b750f150610bb6ba","Repository":"\u003cnone\u003e","SharedSize":"N/A","Size":"1.2GB","Tag":"\u003cnone\u003e","UniqueSize":"N/A"}"#;

    pub const VOLUMES: &str = r#"{"Availability":"N/A","Driver":"local","Group":"N/A","Labels":"com.docker.volume.anonymous=","Links":"N/A","Mountpoint":"/var/lib/docker/volumes/1b441f4d9016a6925b9b642a430c1fff191d8407ef4d1d881991214817a5e9f0/_data","Name":"1b441f4d9016a6925b9b642a430c1fff191d8407ef4d1d881991214817a5e9f0","Scope":"local","Size":"N/A","Status":"N/A"}
{"Availability":"N/A","Driver":"local","Group":"N/A","Labels":"com.docker.volume.anonymous=","Links":"N/A","Mountpoint":"/var/lib/docker/volumes/4fb0c2a2af7e2ae43d18676a16bddf7f4edf73fd34d85f8274d4696a07f3e188/_data","Name":"4fb0c2a2af7e2ae43d18676a16bddf7f4edf73fd34d85f8274d4696a07f3e188","Scope":"local","Size":"N/A","Status":"N/A"}
{"Availability":"N/A","Driver":"local","Group":"N/A","Labels":"","Links":"N/A","Mountpoint":"/var/lib/docker/volumes/shop_pg_data/_data","Name":"shop_pg_data","Scope":"local","Size":"N/A","Status":"N/A"}
{"Availability":"N/A","Driver":"local","Group":"N/A","Labels":"","Links":"N/A","Mountpoint":"/var/lib/docker/volumes/old_cache/_data","Name":"old_cache","Scope":"local","Size":"N/A","Status":"N/A"}"#;

    pub const PROJECTS: &str = r#"[{"Name":"shop","Status":"running(2), exited(1)","ConfigFiles":"C:\\dev\\shop\\docker-compose.yml,C:\\dev\\shop\\docker-compose.local.yml"},{"Name":"blog","Status":"exited(2)","ConfigFiles":"C:\\dev\\blog\\compose.yaml"}]"#;

    /// Everything above, read.
    pub fn lists() -> super::Lists {
        super::Lists {
            containers: super::parse_containers(PS),
            projects: super::parse_projects(PROJECTS),
            images: super::parse_images(IMAGES),
            volumes: super::parse_volumes(VOLUMES),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_are_read_and_sorted_by_project_then_name() {
        let containers = parse_containers(samples::PS);
        let names: Vec<&str> = containers.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["shop-admin", "shop-postgres", "shop-web", "backups"]
        );

        let web = &containers[2];
        assert_eq!(web.project, "shop");
        assert_eq!(web.image, "shop-web");
        assert_eq!(web.state, "running");
        assert_eq!(web.status, "Up 28 seconds (healthy)");
        assert_eq!(web.ports, "8081→80");
        assert!(web.running());
        assert!(web.id.starts_with("ce80c9f75bdd"));

        // The digest is not part of the image's name.
        let postgres = &containers[1];
        assert_eq!(postgres.image, "postgres:16-alpine");
        assert_eq!(postgres.ports, "15432→5432");
        assert_eq!(
            postgres.mounts,
            ["/run/desktop/mnt/host/c/dev/shop/init.sh", "shop_pg_data"]
        );

        // A port that is only exposed is not shown; no project is no project.
        assert_eq!(containers[0].ports, "");
        assert!(!containers[0].running());
        assert_eq!(containers[3].project, "");
    }

    #[test]
    fn a_label_value_with_commas_does_not_hide_the_project() {
        let labels = "a=1,com.docker.compose.project.config_files=C:\\x.yml,C:\\y.yml,\
                      com.docker.compose.project.working_dir=C:\\x,com.docker.compose.project=shop,z=2";
        assert_eq!(project_of(labels), "shop");
        assert_eq!(project_of("maintainer=someone"), "");
        assert_eq!(project_of(""), "");
    }

    #[test]
    fn lines_that_are_not_containers_are_skipped() {
        let output = "WARNING: something\n{\"Names\":\"no-id\"}\n[1,2]\n\n";
        assert!(parse_containers(output).is_empty());
        assert!(parse_images("not json").is_empty());
        assert!(parse_volumes("").is_empty());
        assert!(parse_projects("docker: 'compose' is not a docker command").is_empty());
    }

    #[test]
    fn images_know_their_name_and_who_uses_them() {
        let images = parse_images(samples::IMAGES);
        let containers = parse_containers(samples::PS);
        let names: Vec<String> = images.iter().map(Image::name).collect();
        assert_eq!(
            names,
            [
                "old-admin:latest",
                "postgres:16-alpine",
                "shop-web:latest",
                "<none>"
            ]
        );

        // Docker's own count, where it gives one.
        let web = &images[2];
        assert_eq!(web.containers, 1);
        assert_eq!(web.used_by(&containers), ["shop-web"]);
        assert!(web.in_use(&containers));
        assert_eq!(web.short_id(), "e12f7cf5da4f");
        assert_eq!(web.reference(), "shop-web:latest");

        // No count from Docker: the containers' image names decide.
        let postgres = &images[1];
        assert_eq!(postgres.containers, 0);
        assert_eq!(postgres.used_by(&containers), ["shop-postgres"]);
        assert!(postgres.in_use(&containers));

        assert!(!images[0].in_use(&containers));

        // An image without a name is removed by its ID.
        let dangling = &images[3];
        assert!(dangling.dangling());
        assert_eq!(dangling.reference(), dangling.id);
        assert_eq!(dangling.bytes(), 1_200_000_000);
    }

    #[test]
    fn volumes_know_which_containers_mount_them() {
        let volumes = parse_volumes(samples::VOLUMES);
        let containers = parse_containers(samples::PS);
        let labels: Vec<String> = volumes.iter().map(Volume::label).collect();
        assert_eq!(
            labels,
            ["old_cache", "shop_pg_data", "1b441f4d9016", "4fb0c2a2af7e"]
        );
        assert!(volumes[0].used_by(&containers).is_empty());
        assert_eq!(volumes[1].used_by(&containers), ["shop-postgres"]);
        assert!(volumes[2].anonymous());
        assert_eq!(volumes[2].used_by(&containers), ["backups"]);
        assert!(volumes[3].used_by(&containers).is_empty());
    }

    #[test]
    fn projects_count_what_runs() {
        let projects = parse_projects(samples::PROJECTS);
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "blog");
        assert_eq!((projects[0].running, projects[0].total), (0, 2));
        assert_eq!(projects[0].files, ["C:\\dev\\blog\\compose.yaml"]);
        assert_eq!(projects[1].name, "shop");
        assert_eq!((projects[1].running, projects[1].total), (2, 3));
        assert_eq!(projects[1].files.len(), 2);
        assert_eq!(count_status("running(6)"), (6, 6));
        assert_eq!(count_status(""), (0, 0));
    }

    #[test]
    fn health_checks_are_not_news_and_a_stop_is() {
        assert!(event_matters(
            r#"{"Type":"container","Action":"die","status":"die"}"#
        ));
        assert!(event_matters(r#"{"Type":"image","Action":"delete"}"#));
        assert!(event_matters(r#"{"status":"start","id":"abc"}"#));
        assert!(!event_matters(
            r#"{"Type":"container","Action":"exec_start: /bin/sh -c pg_isready"}"#
        ));
        assert!(!event_matters(
            r#"{"Type":"container","Action":"health_status: healthy"}"#
        ));
        assert!(!event_matters("not json"));
        assert!(!event_matters("{}"));
    }

    #[test]
    fn sizes_are_read_and_written_in_thousands() {
        assert_eq!(parse_size("242MB"), 242_000_000);
        assert_eq!(parse_size("50.8MB"), 50_800_000);
        assert_eq!(parse_size("4.1kB"), 4_100);
        assert_eq!(parse_size("1.2GB"), 1_200_000_000);
        assert_eq!(parse_size("12B"), 12);
        assert_eq!(parse_size("N/A"), 0);
        assert_eq!(format_size(1_492_800_000), "1.5GB");
        assert_eq!(format_size(242_000_000), "242MB");
        assert_eq!(format_size(4_100), "4kB");
        assert_eq!(format_size(12), "12B");
    }
}
