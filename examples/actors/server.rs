use std::{str::from_utf8, sync::Arc};
use async_trait::async_trait;
use axum::Json;
use dapr::server::{actor::{ActorError, context_client::ActorContextClient, Actor, runtime::ActorTypeRegistration}, utils::DaprJson};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct MyResponse {
    pub available: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MyRequest {
    pub name: String,
}

struct MyActor {
    id: String,
    client: ActorContextClient
}

impl MyActor {
    async fn do_stuff(&self, DaprJson(req): DaprJson<MyRequest>) -> Json<MyResponse> {        
        println!("doing stuff with {}", req.name);
        let mut dapr = self.client.clone();
        let r = dapr.get_actor_state("key1").await.unwrap();
        println!("get_actor_state {:?}", r);
        Json(MyResponse { available: true })
    }
}


#[async_trait]
impl Actor for MyActor {
    async fn on_activate(&self) -> Result<(), ActorError> {
        println!("on_activate {}", self.id);
        Ok(())
    }

    async fn on_deactivate(&self) -> Result<(), ActorError> {
        println!("on_deactivate");
        Ok(())
    }

    async fn on_reminder(&self, reminder_name: &str, data: Vec<u8>) -> Result<(), ActorError> {
        println!("on_reminder {} {:?}", reminder_name, from_utf8(&data));
        Ok(())
    }

    async fn on_timer(&self, timer_name: &str, data: Vec<u8>) -> Result<(), ActorError> {
        println!("on_timer {} {:?}", timer_name, from_utf8(&data));
        Ok(())
    }

}

dapr::actor!(MyActor);


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    let mut dapr_server = dapr::server::DaprHttpServer::new().await;
    
    dapr_server.register_actor(ActorTypeRegistration::new::<MyActor>("MyActor", Box::new(|_actor_type, actor_id, context| {
        Arc::new(MyActor {
            id: actor_id.to_string(),
            client: context,
        })}))
        .register_method("do_stuff", MyActor::do_stuff)
        .register_method("do_stuff2", MyActor::do_stuff)).await;
    
    dapr_server.start(None).await?;
        
    Ok(())
}
