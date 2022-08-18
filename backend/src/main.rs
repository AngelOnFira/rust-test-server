use actix_files::Files as Fs;
use actix_multipart::Multipart;
use actix_web::{
    error, get, middleware, post, web, App, Error, HttpRequest, HttpResponse, HttpServer, Result,
};

use entity::job;
use entity::job::Entity as Job;
use futures::{TryStreamExt, StreamExt};
use listenfd::ListenFd;
use migration::{Migrator, MigratorTrait};
use sea_orm::DatabaseConnection;
use sea_orm::{entity::*, query::*};
use serde::{Deserialize, Serialize};
use std::env;

const DEFAULT_POSTS_PER_PAGE: usize = 5;

#[derive(Debug, Clone)]
struct AppState {
    conn: DatabaseConnection,
}

#[derive(Debug, Deserialize)]
pub struct Params {
    page: Option<usize>,
    posts_per_page: Option<usize>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct FlashData {
    kind: String,
    message: String,
}

#[get("/")]
async fn list(req: HttpRequest, data: web::Data<AppState>) -> Result<HttpResponse, Error> {
    let conn = &data.conn;

    // get params
    let params = web::Query::<Params>::from_query(req.query_string()).unwrap();

    let page = params.page.unwrap_or(1);
    let posts_per_page = params.posts_per_page.unwrap_or(DEFAULT_POSTS_PER_PAGE);
    let paginator = Job::find()
        .order_by_asc(job::Column::Id)
        .paginate(conn, posts_per_page);
    let num_pages = paginator.num_pages().await.ok().unwrap();

    let jobs = paginator
        .fetch_page(page - 1)
        .await
        .expect("could not retrieve posts");

    Ok(HttpResponse::Ok()
        .content_type("text/json")
        .body(serde_json::to_string(&jobs)?))
}

// #[get("/new")]
// async fn new(data: web::Data<AppState>) -> Result<HttpResponse, Error> {
//     let template = &data.templates;
//     let ctx = tera::Context::new();
//     let body = template
//         .render("new.html.tera", &ctx)
//         .map_err(|_| error::ErrorInternalServerError("Template error"))?;
//     Ok(HttpResponse::Ok().content_type("text/html").body(body))
// }

#[post("/")]
async fn create(mut payload: Multipart) -> Result<HttpResponse, Error> {
    // iterate over multipart stream
    while let Some(item) = payload.next().await {
        let mut field = item?;

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            println!("-- CHUNK: \n{:?}", std::str::from_utf8(&chunk?));
        }
    }

    Ok(HttpResponse::Ok().into())
}

// #[get("/{id}")]
// async fn edit(data: web::Data<AppState>, id: web::Path<i32>) -> Result<HttpResponse, Error> {
//     let conn = &data.conn;
//     let template = &data.templates;

//     let post: post::Model = Post::find_by_id(id.into_inner())
//         .one(conn)
//         .await
//         .expect("could not find post")
//         .unwrap();

//     let mut ctx = tera::Context::new();
//     ctx.insert("post", &post);

//     let body = template
//         .render("edit.html.tera", &ctx)
//         .map_err(|_| error::ErrorInternalServerError("Template error"))?;
//     Ok(HttpResponse::Ok().content_type("text/html").body(body))
// }

// #[post("/{id}")]
// async fn update(
//     data: web::Data<AppState>,
//     id: web::Path<i32>,
//     post_form: web::Form<post::Model>,
// ) -> Result<HttpResponse, Error> {
//     let conn = &data.conn;
//     let form = post_form.into_inner();

//     post::ActiveModel {
//         id: Set(id.into_inner()),
//         title: Set(form.title.to_owned()),
//         text: Set(form.text.to_owned()),
//     }
//     .save(conn)
//     .await
//     .expect("could not edit post");

//     Ok(HttpResponse::Found()
//         .append_header(("location", "/"))
//         .finish())
// }

async fn not_found(data: web::Data<AppState>, request: HttpRequest) -> Result<HttpResponse, Error> {
    Ok(HttpResponse::NotFound()
        .content_type("text/html")
        .body("Not Found"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    tracing_subscriber::fmt::init();

    // get env vars
    dotenv::dotenv().ok();
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL is not set in .env file");
    let host = env::var("HOST").expect("HOST is not set in .env file");
    let port = env::var("PORT").expect("PORT is not set in .env file");
    let server_url = format!("{}:{}", host, port);

    // establish connection to database and apply migrations
    // -> create post table if not exists
    let conn = sea_orm::Database::connect(&db_url).await.unwrap();

    let jobs = (0..100)
        .into_iter()
        .map(|_| job::ActiveModel {
            code: Set("Test".to_owned()),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    let res = job::Entity::insert_many(jobs).exec(&conn).await;

    Migrator::up(&conn, None).await.unwrap();

    let state = AppState { conn };

    // create server and try to serve over socket if possible
    let mut listenfd = ListenFd::from_env();
    let mut server = HttpServer::new(move || {
        App::new()
            .service(Fs::new("/static", "./static"))
            .app_data(web::Data::new(state.clone()))
            .wrap(middleware::Logger::default()) // enable logger
            .default_service(web::route().to(not_found))
            .configure(init)
    });

    server = match listenfd.take_tcp_listener(0)? {
        Some(listener) => server.listen(listener)?,
        None => server.bind(&server_url)?,
    };

    println!("Starting server at {}", server_url);
    server.run().await?;

    Ok(())
}

pub fn init(cfg: &mut web::ServiceConfig) {
    cfg.service(list);
    // cfg.service(new);
    // cfg.service(create);
    // cfg.service(edit);
    // cfg.service(update);
    // cfg.service(delete);
}
