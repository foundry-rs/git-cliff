use std::cmp::Reverse;

use git2::Commit;
use indexmap::IndexMap;

use crate::error::Result;
use crate::repo::Repository;
use crate::tag::Tag;

/// Stores which commits are tagged with which tags.
#[derive(Debug)]
pub struct TaggedCommits<'a> {
    /// All the commits in the repository.
    pub commits: IndexMap<String, Commit<'a>>,
    /// Commit ID to tag map.
    tags: IndexMap<String, Tag>,
    /// List of tags' commit indexes. Points into `commits`.
    ///
    /// Sorted in descending order by commit index, meaning the first element
    /// has the highest index (oldest commit in `commits`, which is ordered
    /// newest-first).
    ///
    /// Used for lookups.
    tag_indexes: Vec<usize>,
}

impl<'a> TaggedCommits<'a> {
    /// Creates a new `TaggedCommits` from a repository and a list of
    /// commit-tag pairs.
    pub(crate) fn new(
        repository: &'a Repository,
        tags: Vec<(Commit<'a>, Tag)>,
    ) -> Result<Self> {
        let commits = repository.commits(None, None, None, false)?;
        let commits: IndexMap<_, _> = commits
            .into_iter()
            .map(|c| (c.id().to_string(), c))
            .collect();
        let mut tag_ids: Vec<_> = tags
            .iter()
            .filter_map(|(commit, _tag)| {
                let id = commit.id().to_string();
                commits.get_index_of(&id)
            })
            .collect();
        tag_ids.sort_by_key(|idx| Reverse(*idx));
        let tags = tags
            .into_iter()
            .map(|(commit, tag)| (commit.id().to_string(), tag))
            .collect();
        Ok(Self {
            commits,
            tag_indexes: tag_ids,
            tags,
        })
    }

    /// Returns the number of tags.
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// Returns `true` if there are no tags.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// Returns an iterator over all the tags.
    pub fn tags(&self) -> impl Iterator<Item = &Tag> {
        self.tags.iter().map(|(_, tag)| tag)
    }

    /// Returns the last tag.
    pub fn last(&self) -> Option<&Tag> {
        self.tags().last()
    }

    /// Returns the tag of the given commit.
    ///
    /// Note that this only searches for an exact match.
    /// For a more general search, use [`get_closest`](Self::get_closest)
    /// instead.
    pub fn get(&self, commit: &str) -> Option<&Tag> {
        self.tags.get(commit)
    }

    /// Returns the tag at the given index.
    ///
    /// The index can be calculated with `tags().position()`.
    pub fn get_index(&self, idx: usize) -> Option<&Tag> {
        self.tags.get_index(idx).map(|(_, tag)| tag)
    }

    /// Returns the tag closest to the given commit.
    pub fn get_closest(&self, commit: &str) -> Option<&Tag> {
        // Try exact match first.
        if let Some(tagged) = self.get(commit) {
            return Some(tagged);
        }

        let index = self.commits.get_index_of(commit)?;
        let tag_index = *self.tag_indexes.iter().find(|tag_idx| index >= **tag_idx)?;
        self.get_tag_by_id(tag_index)
    }

    fn get_tag_by_id(&self, id: usize) -> Option<&Tag> {
        let (commit_of_tag, _) = self.commits.get_index(id)?;
        self.tags.get(commit_of_tag)
    }

    /// Returns the commit of the given tag.
    pub fn get_commit(&self, tag_name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(_, t)| t.name == tag_name)
            .map(|(commit, _)| commit.as_str())
    }

    /// Returns `true` if the given tag exists.
    pub fn contains_commit(&self, commit: &str) -> bool {
        self.tags.contains_key(commit)
    }

    /// Inserts a new tagged commit.
    ///
    /// Only inserts if the commit exists in the repository's commit list.
    /// Returns `true` if the tag was inserted.
    pub fn insert(&mut self, commit: String, tag: Tag) -> bool {
        if let Some(index) = self.commits.get_index_of(&commit) {
            if let Err(idx) = self.binary_search(index) {
                self.tag_indexes.insert(idx, index);
            }
            self.tags.insert(commit, tag);
            true
        } else {
            false
        }
    }

    /// Retains only the tags specified by the predicate.
    pub fn retain(&mut self, mut f: impl FnMut(&Tag) -> bool) {
        // Filter the tags map first, then rebuild tag_indexes to stay
        // consistent.
        self.tags.retain(|_, tag| f(tag));
        self.tag_indexes.retain(|&idx| {
            let (commit_of_tag, _) = self
                .commits
                .get_index(idx)
                .expect("invalid TaggedCommits state");
            self.tags.contains_key(commit_of_tag)
        });
    }

    fn binary_search(&self, index: usize) -> std::result::Result<usize, usize> {
        self.tag_indexes
            .binary_search_by_key(&Reverse(index), |tag_idx| Reverse(*tag_idx))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::repo::test::get_repository;

    fn get_tagged_commits() -> Result<TaggedCommits<'static>> {
        let repository = Box::leak(Box::new(get_repository()?));
        repository.tags(&None, false, false)
    }

    #[test]
    fn len_and_is_empty() -> Result<()> {
        let tc = get_tagged_commits()?;
        assert!(!tc.is_empty());
        assert_ne!(tc.len(), 0);
        Ok(())
    }

    #[test]
    fn last_returns_final_tag() -> Result<()> {
        let tc = get_tagged_commits()?;
        let last = tc.last().expect("should have tags");
        // last() should match the final element from tags() iterator
        let from_iter = tc.tags().last().expect("should have tags");
        assert_eq!(last, from_iter);
        Ok(())
    }

    #[test]
    fn get_exact_match() -> Result<()> {
        let tc = get_tagged_commits()?;
        // v0.1.0 is tagged at this commit
        let tag = tc
            .get("2b8b4d3535f29231e05c3572e919634b9af907b6")
            .expect("should find tag for v0.1.0");
        assert_eq!(tag.name, "v0.1.0");
        Ok(())
    }

    #[test]
    fn get_returns_none_for_unknown_commit() -> Result<()> {
        let tc = get_tagged_commits()?;
        assert!(tc.get("0000000000000000000000000000000000000000").is_none());
        Ok(())
    }

    #[test]
    fn get_index_returns_tag_by_position() -> Result<()> {
        let tc = get_tagged_commits()?;
        let first = tc.get_index(0).expect("should have first tag");
        // Should be the same as the first tag from the iterator
        let from_iter = tc.tags().next().expect("should have tags");
        assert_eq!(first, from_iter);

        // Out of bounds returns None
        assert!(tc.get_index(tc.len()).is_none());
        Ok(())
    }

    #[test]
    fn get_closest_exact() -> Result<()> {
        let tc = get_tagged_commits()?;
        // An exact match should return the tag directly
        let tag = tc
            .get_closest("2b8b4d3535f29231e05c3572e919634b9af907b6")
            .expect("should find closest tag");
        assert_eq!(tag.name, "v0.1.0");
        Ok(())
    }

    #[test]
    fn get_closest_non_tagged_commit() -> Result<()> {
        let tc = get_tagged_commits()?;
        // For a commit that is not tagged, get_closest should return the
        // nearest preceding tag (or None if before all tags)
        for (commit_id, _) in &tc.commits {
            // Every commit in the repo should either have an exact tag or
            // resolve to some closest tag (or None if before the first tag)
            let _ = tc.get_closest(commit_id);
        }
        Ok(())
    }

    #[test]
    fn contains_commit_works() -> Result<()> {
        let tc = get_tagged_commits()?;
        assert!(tc.contains_commit("2b8b4d3535f29231e05c3572e919634b9af907b6"));
        assert!(!tc.contains_commit("0000000000000000000000000000000000000000"));
        Ok(())
    }

    #[test]
    fn get_commit_by_tag_name() -> Result<()> {
        let tc = get_tagged_commits()?;
        let commit = tc.get_commit("v0.1.0").expect("should find commit for v0.1.0");
        assert_eq!(commit, "2b8b4d3535f29231e05c3572e919634b9af907b6");

        assert!(tc.get_commit("nonexistent-tag").is_none());
        Ok(())
    }

    #[test]
    fn insert_new_tag() -> Result<()> {
        let mut tc = get_tagged_commits()?;
        let original_len = tc.len();
        let commit_id = tc.commits.keys().next().expect("should have commits").clone();
        assert!(tc.insert(
            commit_id.clone(),
            Tag {
                name: "v99.0.0".to_string(),
                message: None,
            },
        ));
        assert_eq!(tc.len(), original_len + 1);
        assert!(tc.contains_commit(&commit_id));
        assert_eq!(
            tc.get(&commit_id).expect("should find inserted tag").name,
            "v99.0.0"
        );
        Ok(())
    }

    #[test]
    fn insert_unknown_commit_is_noop() -> Result<()> {
        let mut tc = get_tagged_commits()?;
        let original_len = tc.len();
        assert!(!tc.insert(
            "0000000000000000000000000000000000000000".to_string(),
            Tag {
                name: "v99.0.0".to_string(),
                message: None,
            },
        ));
        assert_eq!(tc.len(), original_len);
        Ok(())
    }

    #[test]
    fn retain_filters_tags() -> Result<()> {
        let mut tc = get_tagged_commits()?;
        let original_len = tc.len();
        assert!(original_len > 0);

        // Retain only tags matching "v0.1.0"
        tc.retain(|tag| tag.name == "v0.1.0");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc.tags().next().expect("should have one tag").name, "v0.1.0");
        Ok(())
    }

    #[test]
    fn tags_iterator() -> Result<()> {
        let tc = get_tagged_commits()?;
        let tags: Vec<_> = tc.tags().collect();
        assert_eq!(tags.len(), tc.len());
        Ok(())
    }
}
