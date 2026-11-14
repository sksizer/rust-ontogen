pub mod conversions;
pub mod entities;

use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Schema};

use crate::schema::AppError;

/// Create every table the log needs, parents first.
///
/// ontogen emits entities but no table creation, so this list is hand-kept:
/// a new entity or `many_to_many` that isn't added here compiles and then
/// fails at its first query.
pub async fn create_tables(db: &DatabaseConnection) -> Result<(), AppError> {
    async fn create<E: EntityTrait>(db: &DatabaseConnection, entity: E) -> Result<(), AppError> {
        let schema = Schema::new(db.get_database_backend());
        let mut statement = schema.create_table_from_entity(entity);
        statement.if_not_exists();
        db.execute(db.get_database_backend().build(&statement)).await.map_err(|e| AppError::DbError(e.to_string()))?;
        Ok(())
    }
    use entities::*;
    create(db, person::Entity).await?;
    create(db, activity_kind::Entity).await?;
    create(db, muscle_group::Entity).await?;
    create(db, muscle::Entity).await?;
    create(db, muscle_group_members::Entity).await?;
    create(db, equipment::Entity).await?;
    create(db, exercise_family::Entity).await?;
    create(db, exercise::Entity).await?;
    create(db, exercise_muscle::Entity).await?;
    create(db, location::Entity).await?;
    create(db, tag::Entity).await?;
    create(db, session::Entity).await?;
    create(db, session_tags::Entity).await?;
    create(db, set_group::Entity).await?;
    create(db, effort::Entity).await?;
    create(db, lifting_facet::Entity).await?;
    create(db, reps_facet::Entity).await?;
    create(db, distance_facet::Entity).await?;
    create(db, note::Entity).await?;
    Ok(())
}
