use std::path::Path;

use crate::err::Result;

use serde::Deserialize;
use tantivy::{
	collector::{FilterCollector, TopDocs},
	columnar::HasAssociatedColumnType,
	query::QueryParser,
	schema::*,
	Document,
};
use tempfile::TempDir;

#[derive(Clone)]
pub struct Searcher {
	pub index: tantivy::Index,

	// tantivy index schema
	pub field_username: tantivy::schema::Field,
	pub field_feed_id: tantivy::schema::Field,
	pub field_id: tantivy::schema::Field,
	pub field_title: tantivy::schema::Field,
	pub field_summary: tantivy::schema::Field,
	pub field_content: tantivy::schema::Field,
}

#[derive(Deserialize)]
pub struct SearchQuery {
	text: String,
	offset: usize,
	limit: usize,
	order_by: Option<String>,
	order: Option<Order>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
	Asc,
	Desc,
}

impl Searcher {
	pub fn new(path: impl AsRef<Path>) -> Result<Self> {
		let mut schema_builder = SchemaBuilder::new();
		let field_username = schema_builder.add_text_field("username", TEXT | FAST | STORED);
		let field_feed_id = schema_builder.add_u64_field("feed_id", INDEXED | FAST | STORED);
		let field_id = schema_builder.add_text_field("id", TEXT | FAST | STORED);
		let field_title = schema_builder.add_text_field("title", TEXT | FAST);
		let field_summary = schema_builder.add_text_field("summary", TEXT | FAST);
		let field_content = schema_builder.add_text_field("content", TEXT | FAST);

		let schema = schema_builder.build();
		let index = tantivy::Index::create_in_dir(&path, schema.clone())?;

		Ok(Searcher {
			field_username,
			field_feed_id,
			field_id,
			field_title,
			field_summary,
			field_content,
			index,
		})
	}

	pub fn search(&self, username: &str, query_request: SearchQuery) -> Result<Vec<Document>> {
		let searcher = self.index.reader()?.searcher();
		let query_parser = QueryParser::for_index(
			&self.index,
			vec![self.field_title, self.field_summary, self.field_content],
		);

		let query = query_parser.parse_query(&query_request.text)?;

		let filter_username = |Username(_username): Username| _username == username;

		let search_result: Vec<_> = match query_request.order_by {
			Some(order_by) => {
				let collector = TopDocs::with_limit(query_request.limit)
					.and_offset(query_request.offset)
					.order_by_fast_field::<f64>(order_by);
				let collector =
					FilterCollector::new(self.field_username, filter_username, collector);

				searcher
					.search(&query, &collector)?
					.into_iter()
					.map(|(_, doc)| doc)
					.collect()
			}
			None => {
				let collector =
					TopDocs::with_limit(query_request.limit).and_offset(query_request.offset);

				searcher
					.search(&query, &collector)?
					.into_iter()
					.map(|(_, doc)| doc)
					.collect()
			}
		};

		let mut vec = Vec::new();
		for doc_address in search_result {
			let retrieved_doc = searcher.doc(doc_address)?;
			vec.push(retrieved_doc);
		}

		Ok(vec)
	}
}
