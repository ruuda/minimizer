use git2::{BranchType, Repository};

type Result<T> = std::result::Result<T, git2::Error>;

fn minimize(repo: &mut Repository) -> Result<()> {
    let pages_branch = repo.find_branch("gh-pages", BranchType::Local)?;
    println!("Branch: {:?}", pages_branch.get().target());
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args();
    // Skip the program name.
    args.next();

    let repo_path = args.next().expect("Expected repository path.");

    let mut repo = Repository::open(repo_path)?;
    minimize(&mut repo)?;

    Ok(())
}
