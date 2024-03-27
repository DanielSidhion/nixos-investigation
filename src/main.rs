use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::Write,
    path::PathBuf,
    process::Command,
};

use clap::Parser;

type GenericResult<T> = Result<T, Box<dyn std::error::Error + 'static>>;

#[derive(Clone)]
struct Package {
    level: usize,

    path: String,
    size_bytes: usize,
    dependencies: Vec<usize>,
    used_by: Vec<usize>,

    // Smallest name to distinguish this package.
    short_name: String,
    // Will be used when generating a graphviz file.
    graph_size: f32,
}

impl Package {
    pub fn new(path: String) -> GenericResult<Self> {
        let size_output = Command::new("nix-store")
            .arg("--query")
            .arg("--size")
            .arg(&path)
            .output()?
            .stdout;
        let size_str = std::str::from_utf8(&size_output)?.trim();
        let size_bytes: usize = size_str.parse()?;

        Ok(Self {
            level: 0,

            size_bytes,
            dependencies: Vec::new(),
            used_by: Vec::new(),

            graph_size: 0.5,
            short_name: path.clone(),
            path,
        })
    }

    fn add_dependency(&mut self, pos: usize) {
        self.dependencies.push(pos);
    }

    fn add_used_by(&mut self, pos: usize) {
        self.used_by.push(pos);
    }
}

struct PackageTree {
    // Packages are kept in a Vec as an Arena-style system. Pointers to packages will be done by their position in this Vec.
    nodes: Vec<Package>,
    // The idea of organising packages in levels comes from https://github.com/craigmbooth/nix-visualize. It helps generate nicer graphviz visualisations, but nix-visualize still does it better.
    // Read the documentation of nix-visualize to understand the idea behind levels.
    by_level: Vec<Vec<usize>>,
}

impl PackageTree {
    pub fn new(root: Package) -> Self {
        Self {
            nodes: vec![root],
            by_level: Vec::new(),
        }
    }
    pub fn package(&self, pos: usize) -> &Package {
        &self.nodes[pos]
    }

    pub fn package_mut(&mut self, pos: usize) -> &mut Package {
        &mut self.nodes[pos]
    }

    pub fn add_package(&mut self, package: Package) -> usize {
        let pos = self.nodes.len();
        self.nodes.push(package);
        pos
    }

    pub fn register_dependency(&mut self, package_pos: usize, depends_pos: usize) {
        self.package_mut(package_pos).add_dependency(depends_pos);
        self.package_mut(depends_pos).add_used_by(package_pos);

        // We'll move this package to the highest level it should be at.
        let max_parent_level = self
            .package(depends_pos)
            .used_by
            .iter()
            .map(|&pos| self.nodes[pos].level)
            .max()
            .unwrap();
        self.package_mut(depends_pos).level = max_parent_level + 1;
    }

    pub fn find_path_pos(&self, path: &str) -> usize {
        self.nodes
            .iter()
            .enumerate()
            .find(|(_, pkg)| pkg.path == path)
            .unwrap()
            .0
    }

    pub fn calculate_graph_properties(&mut self) {
        let mut graph_names: HashMap<String, usize> = HashMap::new();

        let mut smallest_size_bytes = usize::MAX;
        let mut largest_size_bytes = usize::MIN;
        let mut largest_level = usize::MIN;

        for (pos, pkg) in self.nodes.iter().enumerate() {
            smallest_size_bytes = smallest_size_bytes.min(pkg.size_bytes);
            largest_size_bytes = largest_size_bytes.max(pkg.size_bytes);
            largest_level = largest_level.max(pkg.level);

            let (_, symbolic_name) = pkg
                .path
                .split_at("/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-".len());
            if graph_names.contains_key(symbolic_name) {
                // Must remove that package and add the full store path.
                let other_pos = graph_names.remove(symbolic_name).unwrap();
                let other = &self.nodes[other_pos];

                graph_names.insert(
                    other.path.trim_start_matches("/nix/store/").to_string(),
                    other_pos,
                );
                graph_names.insert(pkg.path.trim_start_matches("/nix/store/").to_string(), pos);
            } else {
                graph_names.insert(symbolic_name.to_string(), pos);
            }
        }

        for _ in 0..(largest_level + 1) {
            self.by_level.push(Vec::new());
        }

        for (pos, pkg) in self.nodes.iter_mut().enumerate() {
            pkg.dependencies.sort();
            self.by_level[pkg.level].push(pos);

            pkg.graph_size = 0.2
                + 2.0
                    * ((pkg.size_bytes - smallest_size_bytes) as f32
                        / (largest_size_bytes - smallest_size_bytes) as f32)
                        .min(1.0)
                        .max(0.0);
        }

        for (name, pos) in graph_names.into_iter() {
            self.nodes[pos].short_name = name;
        }
    }

    pub fn sum_package_bytes(&self) -> usize {
        self.nodes.iter().map(|pkg| pkg.size_bytes).sum()
    }
}

fn process_lines(
    tree: &mut PackageTree,
    parent_pos: usize,
    mut lines: VecDeque<&str>,
) -> GenericResult<()> {
    while let Some(line) = lines.pop_front() {
        if let Some(object_path) = line.strip_prefix("├").or_else(|| line.strip_prefix("└")) {
            let object_path = object_path.trim_start_matches("─");

            if !object_path.starts_with("/") {
                return Err(format!("When parsing the output of nix-store, we found a store path with unexpected format: {}", object_path).into());
            }

            if object_path.ends_with("[...]") {
                // Means we already processed this path, so we can just find it in the package tree.
                let object_path = object_path.strip_suffix("[...]").unwrap().trim();
                let object_pos = tree.find_path_pos(object_path);
                if object_pos != parent_pos {
                    tree.register_dependency(parent_pos, object_pos);
                }
            } else {
                // We have to process this new path.
                let new_package = Package::new(object_path.into())?;
                let pos = tree.add_package(new_package);
                tree.register_dependency(parent_pos, pos);

                // Dive into children now. We'll grab all the lines for it and then process them.
                let mut child_lines = VecDeque::new();

                while let Some(&child_line) = lines.get(0) {
                    if let Some(child_line) = child_line
                        .strip_prefix("│")
                        .or_else(|| child_line.strip_prefix(" "))
                    {
                        child_lines.push_back(child_line.trim_start_matches(" "));
                        lines.pop_front();
                    } else {
                        break;
                    }
                }

                process_lines(tree, pos, child_lines)?;
            }
        } else {
            return Err("We found an unexpected line when parsing the output of nix-store".into());
        }
    }

    Ok(())
}

/// This attempts to generate a dot file with some restrictions to coerce graphviz into generating a graph that won't look super hard to read.
/// If none of these restrictions are added, the edges will be way too close to each other, making it impossible to follow any edge in particular.
/// A side-effect of the restrictions is that the graph generated is huge for closures that are large enough.
fn generate_dot_file(tree: &PackageTree, file_path: &PathBuf) -> std::io::Result<()> {
    let mut file = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(file_path)?;
    file.write_all(b"digraph {\n")?;

    for (pos, pkg) in tree.nodes.iter().enumerate() {
        file.write_all(
            format!(
                "{} [fixedsize = true, height = {:.3}, width = {:.3}, penwidth = 2, label = \"{}\"];\n",
                pos, pkg.graph_size, pkg.graph_size, pkg.short_name
            )
            .as_bytes(),
        )?;

        for dep in pkg.dependencies.iter() {
            file.write_all(format!("{} -> {} [penwidth = 0.5];\n", pos, *dep).as_bytes())?;
        }
    }

    let mut level_node_hierarchy: Vec<String> = Vec::new();

    for level in 0..tree.by_level.len() {
        let total_elements = tree.by_level.len();
        let chunk_size = 1 + total_elements / (1 + total_elements / 20);
        let chunk_size = chunk_size.max(20);

        for (sublevel, chunk) in tree.by_level[level].chunks(chunk_size).enumerate() {
            file.write_all(
                format!("subgraph level_{}_{} {{\nrank = same;\n", level, sublevel).as_bytes(),
            )?;

            for &pos in chunk {
                file.write_all(format!("{}; ", pos).as_bytes())?;
            }

            file.write_all(
                format!("lnode{}_{} [style=\"invis\"];\n}}\n", level, sublevel).as_bytes(),
            )?;
            level_node_hierarchy.push(format!("lnode{}_{}", level, sublevel));
        }
    }

    for edge in level_node_hierarchy.windows(2) {
        file.write_all(format!("{} -> {} [style=\"invis\"];\n", edge[0], edge[1]).as_bytes())?;
    }

    file.write_all(b"}\n")?;
    file.flush()?;

    Ok(())
}

fn generate_package_list(tree: &PackageTree, file_path: &PathBuf) -> std::io::Result<()> {
    let mut file = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(file_path)?;

    file.write_all(b"pos,level,package_name,size_bytes,dependencies,path\n")?;

    for level in 0..tree.by_level.len() {
        for &pkg_pos in tree.by_level[level].iter() {
            let pkg = tree.package(pkg_pos);

            file.write_all(
                format!(
                    "{},{},{},{},\"{}\",{}\n",
                    pkg_pos,
                    level,
                    pkg.short_name,
                    pkg.size_bytes,
                    pkg.dependencies
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    pkg.path
                )
                .as_bytes(),
            )?;
        }
    }

    file.flush()?;

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    store_path: PathBuf,

    /// Path to the graphviz dot file to generate.
    /// If not specified, no dot file will be generated.
    #[arg(short, long)]
    dot_file_path: Option<PathBuf>,

    /// Path to the csv file to generate.
    /// If not specified, no csv file will be generated.
    #[arg(short, long)]
    csv_file_path: Option<PathBuf>,
}

fn main() -> GenericResult<()> {
    let args = Args::parse();

    let tree_output = Command::new("nix-store")
        .arg("--query")
        .arg("--tree")
        .arg(args.store_path)
        .output()?
        .stdout;
    let tree_output = std::str::from_utf8(&tree_output)?;

    let mut lines = tree_output.lines();
    let mut tree: PackageTree;
    let root_path = lines.next().unwrap();
    if root_path.starts_with("/") {
        let root = Package::new(root_path.into())?;
        tree = PackageTree::new(root);
    } else {
        return Err("Got an unexpected output from 'nix-store --query --tree'!".into());
    }

    process_lines(&mut tree, 0, lines.collect())?;
    tree.calculate_graph_properties();

    if let Some(path) = args.dot_file_path {
        generate_dot_file(&tree, &path)?;
    }

    if let Some(path) = args.csv_file_path {
        generate_package_list(&tree, &path)?;
    }

    println!(
        "Total bytes calculated for this store path: {}",
        tree.sum_package_bytes()
    );

    Ok(())
}
