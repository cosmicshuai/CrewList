//! Mongo: the task payload, with drift rejected by the database itself.

use mongodb::bson::{doc, Document};
use mongodb::error::ErrorKind;
use mongodb::{Client, Database};

use crate::StoreError;

pub const DB_NAME: &str = "crewlist";
pub const DETAILS_COLLECTION: &str = "task_details";

/// Mongo's "collection already exists".
const NAMESPACE_EXISTS: i32 = 48;

#[derive(Clone)]
pub struct MongoStore {
    db: Database,
}

impl MongoStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let client = Client::with_uri_str(url)
            .await
            .map_err(|e| StoreError::Connect {
                store: "mongo",
                source: Box::new(e),
            })?;

        Ok(Self {
            db: client.database(DB_NAME),
        })
    }

    /// Creates the collection, installs the schema validator, and ensures the
    /// unique index. Idempotent — `create` tolerates an existing namespace and
    /// `collMod` then re-applies the validator, so a restart also *upgrades* a
    /// stale validator rather than leaving it behind. AC-59.
    pub async fn initialize(&self) -> Result<(), StoreError> {
        match self
            .db
            .run_command(doc! { "create": DETAILS_COLLECTION })
            .await
        {
            Ok(_) => tracing::info!(collection = DETAILS_COLLECTION, "collection created"),
            Err(e) if is_namespace_exists(&e) => {
                tracing::debug!(collection = DETAILS_COLLECTION, "collection already exists");
            }
            Err(e) => {
                return Err(StoreError::Init {
                    store: "mongo",
                    source: Box::new(e),
                })
            }
        }

        self.run(doc! {
            "collMod": DETAILS_COLLECTION,
            "validator": detail_validator(),
            "validationLevel": "strict",
            "validationAction": "error",
        })
        .await?;

        self.run(doc! {
            "createIndexes": DETAILS_COLLECTION,
            "indexes": [
                { "key": { "task_id": 1 }, "name": "task_id_unique", "unique": true }
            ],
        })
        .await?;

        tracing::info!("mongo schema validator and indexes applied");
        Ok(())
    }

    pub async fn ping(&self) -> Result<(), StoreError> {
        self.db
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| StoreError::Query {
                store: "mongo",
                source: Box::new(e),
            })?;
        Ok(())
    }

    /// Escape hatch for the query layer, which arrives with the handlers.
    pub fn database(&self) -> &Database {
        &self.db
    }

    async fn run(&self, command: Document) -> Result<(), StoreError> {
        self.db
            .run_command(command)
            .await
            .map_err(|e| StoreError::Init {
                store: "mongo",
                source: Box::new(e),
            })?;
        Ok(())
    }
}

fn is_namespace_exists(err: &mongodb::error::Error) -> bool {
    matches!(*err.kind, ErrorKind::Command(ref e) if e.code == NAMESPACE_EXISTS)
}

/// The `$jsonSchema` behind SPEC.md §5.2.
///
/// "Fixed schema" means the database rejects drift, not that the application
/// promises to behave: `additionalProperties: false` at the root and inside
/// every array element, with types pinned throughout.
///
/// Note for the write path: timestamps are `bsonType: "date"`, so the detail
/// structs must serialize through `bson::serde_helpers::chrono_datetime_as_bson_datetime`.
/// Serde's default for `chrono::DateTime` is an ISO *string*, which this
/// validator will reject — by design, and AC-50 pins that behavior.
fn detail_validator() -> Document {
    doc! {
        "$jsonSchema": {
            "bsonType": "object",
            "required": ["task_id", "schema_version", "created_at", "updated_at"],
            "additionalProperties": false,
            "properties": {
                "_id": { "bsonType": "objectId" },
                "task_id": { "bsonType": "long" },
                "schema_version": { "bsonType": "int", "minimum": 1 },
                "description": { "bsonType": "string" },
                "summary": { "bsonType": ["string", "null"] },
                "created_at": { "bsonType": "date" },
                "updated_at": { "bsonType": "date" },
                "notes": {
                    "bsonType": "array",
                    "items": {
                        "bsonType": "object",
                        "required": ["author", "body", "at"],
                        "additionalProperties": false,
                        "properties": {
                            "author": { "bsonType": "string" },
                            "body": { "bsonType": "string" },
                            "at": { "bsonType": "date" },
                        },
                    },
                },
                "sources": {
                    "bsonType": "array",
                    "items": {
                        "bsonType": "object",
                        "required": ["url", "retrieved_at"],
                        "additionalProperties": false,
                        "properties": {
                            "url": { "bsonType": "string" },
                            "title": { "bsonType": ["string", "null"] },
                            "retrieved_at": { "bsonType": "date" },
                        },
                    },
                },
                "contacts": {
                    "bsonType": "array",
                    "items": {
                        "bsonType": "object",
                        "required": ["name"],
                        "additionalProperties": false,
                        "properties": {
                            "name": { "bsonType": "string" },
                            "phone": { "bsonType": ["string", "null"] },
                            "email": { "bsonType": ["string", "null"] },
                            "url": { "bsonType": ["string", "null"] },
                        },
                    },
                },
            },
        }
    }
}
