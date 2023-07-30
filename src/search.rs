use crate::err::Result;

use serde::Deserialize;
use tantivy::{collector::TopDocs, query::QueryParser, Document};
use tempfile::TempDir;

pub struct Searcher {
	// tantivy index schema
	pub field_id: tantivy::schema::Field,
	pub field_title: tantivy::schema::Field,
	pub field_summary: tantivy::schema::Field,
	pub field_content: tantivy::schema::Field,

	// search setup
	pub index_path: TempDir,
	pub schema: tantivy::schema::Schema,
	pub index: tantivy::Index,
	pub index_writer: tokio::sync::Mutex<tantivy::IndexWriter>,
	pub index_reader: tantivy::IndexReader,
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
	pub fn search(&self, query_request: SearchQuery) -> Result<Vec<Document>> {
		let searcher = self.index_reader.searcher();
		let query_parser = QueryParser::for_index(
			&self.index,
			vec![self.field_title, self.field_summary, self.field_content],
		);

		let query = query_parser.parse_query(&query_request.text)?;

		let search_result: Vec<_> = match query_request.order_by {
			Some(order_by) => {
				let collector = TopDocs::with_limit(query_request.limit)
					.and_offset(query_request.offset)
					.order_by_fast_field::<f64>(order_by);

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
