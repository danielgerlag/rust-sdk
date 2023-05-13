use futures::Future;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex}, pin::Pin,
};

use super::{
    context_client::{ActorContextClient, DaprActorInterface},
    Actor, ActorError, ActorFactory, ActorInstance, ActorMethod, DecoratedActorMethod,
};

pub struct ActorTypeRegistration<TClient>
where
    TClient: DaprActorInterface,
    TClient: Clone,
{
    name: String,
    factory: ActorFactory<TClient>,
    methods: HashMap<String, Arc<Pin<Box<ActorMethod>>>>,
}

impl<TClient> ActorTypeRegistration<TClient>
where
    TClient: DaprActorInterface,
    TClient: Clone,
{
    pub fn new(name: &str, factory: impl Fn(String, String, Box<ActorContextClient<TClient>>) -> Box<dyn Actor> + 'static,
    ) -> Self {
        ActorTypeRegistration {
            name: name.to_string(),
            factory: Box::new(factory),
            methods: HashMap::new(),
        }
    }

    pub fn register_method<TActor, TInput, TMethod, TOutput, TFuture>(mut self, method_name: &str, method: TMethod) -> Self
    where
        TActor: Actor + Unpin + 'static,
        TInput: for<'a> Deserialize<'a> + 'static,
        TOutput: Serialize + 'static,
        TFuture: Future<Output = Result<TOutput, ActorError>> + Sized + Unpin + 'static,
        TMethod: Fn(&mut TActor, &TInput) -> TFuture + 'static
    {
            let m2 = Arc::new(Mutex::new(method));
            let decorated_method = move |actor: Arc<Mutex<Box<dyn Actor>>>, data: Vec<u8>| -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>> {
            
                //let actor2 = actor.lock().unwrap();

                //let well_known_actor = unsafe { &mut *(actor as *mut dyn Actor as *mut TActor) };
                //let well_known_actor = unsafe { &mut *(&actor as *mut Arc<Mutex<dyn Actor>> as *mut Arc<Mutex<TActor>>) };
                //let a = *well_known_actor;
                
                let fm = DecoratedActorMethod {
                    input: None,
                    method_future: None,
                    method: m2.clone(),
                    actor: actor,
                    serialized_input: Box::new(data),
                    _phantom: std::marker::PhantomData::<TActor>::default()
                };            
                //let fm3 = unsafe { *(&fm as *const dyn Future<Output = Result<Vec<u8>, ActorError>>) };
                
                let b1 = Box::pin(fm);
                let c = b1 as Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>>;

                //let fm2: &(dyn Future<Output = Result<Vec<u8>, ActorError>>) = &fm as &dyn Future<Output = Result<Vec<u8>, ActorError>>;
                
                //Box::pin(fm2)
                c
        };
        //let d2: &'b dyn Fn(&'b mut dyn Actor, &'b Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>> + 'b>> = &decorated_method;
        let etf = Box::pin(decorated_method);
        
        //let decorated_method = &f;
        
        //let decorated_method = DecoratedActorMethod::factory(method);
        self.methods
            .insert(method_name.to_string(), Arc::new(etf));
        self
    }

    fn create_actor(&self, actor_type: &str, actor_id: &str, client: Box<ActorContextClient<TClient>>) -> ActorInstance {
        let actor = (self.factory)(actor_type.to_string(), actor_id.to_string(), client);
        Arc::new(Mutex::new(actor))
    }

    async fn invoke_method(&self, actor: Arc<Mutex<Box<dyn Actor>>>, method_name: &str, data: Vec<u8>) -> Result<Vec<u8>, ActorError> {
        let method = match self.methods.get(method_name) {
            Some(m) => m,
            None => return Err(ActorError::MethodNotFound),
        };        
        let m = method.as_ref().as_ref();
        
        m(actor, data).await
    }
}

pub struct ActorRuntime<TClient>
where
    TClient: DaprActorInterface,
    TClient: Clone,
{
    inner_channel: TClient,
    client_factory: Box<dyn Fn(TClient, &str, &str) -> ActorContextClient<TClient>>,
    registered_actors_types: HashMap<String, ActorTypeRegistration<TClient>>,
    active_actors: HashMap<(String, String), ActorInstance>,
}

unsafe impl<TClient: DaprActorInterface> Send for ActorRuntime<TClient>
where
    TClient: DaprActorInterface,
    TClient: Clone,
{
}

impl<TClient> ActorRuntime<TClient>
where
    TClient: DaprActorInterface,
    TClient: Clone,
{
    pub fn new(
        channel: TClient,
        client_factory: Box<dyn Fn(TClient, &str, &str) -> ActorContextClient<TClient>>,
    ) -> Self {
        ActorRuntime {
            inner_channel: channel,
            client_factory,
            registered_actors_types: HashMap::new(),
            active_actors: HashMap::new(),
        }
    }

    pub fn register_actor(&mut self, registration: ActorTypeRegistration<TClient>) {
        let name = registration.name.clone();
        self.registered_actors_types.insert(name, registration);
    }

    pub async fn invoke_actor(&mut self, actor_type: &str, id: &str, method: &str, data: Vec<u8>) -> Result<Vec<u8>, ActorError> {
        let actor = self.get_or_create_actor(actor_type, id).await?;
        //actor.l
        
        //mg.
        //let actor2 = mg.as_deref_mut().unwrap();
        
        let reg = match self.registered_actors_types.get(actor_type) {
            Some(reg) => reg,
            None => return Err(ActorError::ActorNotFound),
        };

        reg.invoke_method(actor, method, data).await
    }

    pub async fn deactivate_actor(&mut self, name: &str, id: &str) -> Result<(), ActorError> {
        let actor = match self
            .active_actors
            .remove(&(name.to_string(), id.to_string()))
        {
            Some(actor_ref) => actor_ref,
            None => return Err(ActorError::ActorNotFound),
        };
        let mut actor = actor.lock().unwrap();
        actor.on_deactivate()?;
        drop(actor);
        Ok(())
    }

    pub fn deactivate_all(&mut self) {
        for actor in self.active_actors.values() {
            let mut actor = actor.lock().unwrap();
            actor.on_deactivate();
        }
        self.active_actors.clear();
    }

    pub async fn invoke_reminder(&mut self, name: &str, id: &str, reminder_name: &str, data: Vec<u8>) -> Result<(), ActorError> {
        let actor = self.get_or_create_actor(name, id).await?;
        let mut actor = actor.lock().unwrap();
        actor.on_reminder(reminder_name, data)?;
        Ok(())
    }

    pub async fn invoke_timer(&mut self, name: &str, id: &str, timer_name: &str, data: Vec<u8>) -> Result<(), ActorError> {
        let actor = self.get_or_create_actor(name, id).await?;
        let mut actor = actor.lock().unwrap();
        actor.on_timer(timer_name, data)?;
        Ok(())
    }

    pub fn list_registered_actors(&self) -> Vec<String> {
        self.registered_actors_types
            .keys()
            .map(|k| k.to_string())
            .collect()
    }

    async fn get_or_create_actor(&mut self, actor_type: &str, id: &str) -> Result<ActorInstance, ActorError> {
        match self
            .active_actors
            .get(&(actor_type.to_string(), id.to_string()))
        {
            Some(actor_ref) => Ok(actor_ref.clone()),
            None => self.activate_actor(actor_type, id).await,
        }
    }

    async fn activate_actor(&mut self, actor_type: &str, id: &str) -> Result<ActorInstance, ActorError> {
        let actor = match self.registered_actors_types.get(actor_type) {
            Some(f) => {
                let cc = self.client_factory.as_ref();
                let client = Box::new(cc(self.inner_channel.clone(), actor_type, id));
                f.create_actor(actor_type, id, client)
            }
            None => Err(ActorError::NotRegistered)?,
        };

        let actor_key = (actor_type.to_string(), id.to_string());
        self.active_actors.insert(actor_key, actor.clone());

        match actor.lock() {
            Ok(mut a) => a.on_activate()?,
            Err(_) => Err(ActorError::CorruptedState)?,
        };

        Ok(actor)
    }
}

impl<TClient> Drop for ActorRuntime<TClient>
where
    TClient: DaprActorInterface,
    TClient: Clone,
{
    fn drop(&mut self) {
        self.deactivate_all();
    }
}

#[cfg(test)]
mod tests;
