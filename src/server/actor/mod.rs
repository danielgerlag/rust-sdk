use std::{sync::Arc, sync::Mutex, error::Error, pin::Pin, task::Poll};
use futures::{Future, FutureExt, future::BoxFuture};
use serde::{Serialize, Deserialize};

use self::context_client::{ActorContextClient};

pub mod context_client;
pub mod runtime;

pub type ActorInstance = Arc<Mutex<Box<dyn Actor>>>;
pub type ActorFactory<TActorClient> = Box<dyn Fn(String, String, Box<ActorContextClient<TActorClient>>) -> Box<dyn Actor>>;

#[derive(Debug)]
pub enum ActorError {
    NotRegistered,
    CorruptedState,
    MethodNotFound,
    ActorNotFound,
    MethodError(Box<dyn Error>),
    SerializationError()
}

unsafe impl Send for ActorError {}

pub trait Actor: Send + Sync {
    fn on_activate(&mut self) -> Result<(), ActorError>;
    fn on_deactivate(&mut self) -> Result<(), ActorError>;
    fn on_reminder(&mut self, _reminder_name: &str, _data : Vec<u8>) -> Result<(), ActorError>;
    fn on_timer(&mut self, _timer_name: &str, _data : Vec<u8>) -> Result<(), ActorError>;
}


//pub type ActorMethod = dyn Fn(Arc<Mutex<Box<dyn Actor>>>, Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>>;

pub trait ActorMethod {
    fn build(&self, actor: Arc<Mutex<Box<dyn Actor>>>, data: Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>>;    
}

struct ActorMethodContainer<TActor, TInput, TMethod, TOutput, TFuture>  
where 
    TActor: Actor + Unpin, 
    TInput: for<'a> Deserialize<'a>, 
    TOutput: Serialize,
    TFuture: Future<Output = Result<TOutput, ActorError>> + Sized + Send,
    TMethod: for<'a>Fn(&'a mut TActor, &'a TInput) -> TFuture
{
    method: Arc<Mutex<TMethod>>,

    _actor: std::marker::PhantomData<TActor>,
    _input: std::marker::PhantomData<TInput>,
    _output: std::marker::PhantomData<TOutput>,
    //_lifetime: std::marker::PhantomData<&'b ()>,
    //_future: std::marker::PhantomData<TFuture>,
}

//pub type ActorMethodSig<TActor, TInput, TOutput> = dyn for<'a>Fn(&'a mut TActor, &'a TInput) -> dyn Future<Output = Result<TOutput, ActorError>>;



impl<TActor, TInput, TMethod, TOutput, TFuture> ActorMethodContainer<TActor, TInput, TMethod, TOutput, TFuture>  
where 
    TActor: Actor + Unpin, 
    TInput: for<'a> Deserialize<'a>, 
    TOutput: Serialize,
    TFuture: Future<Output = Result<TOutput, ActorError>> + Sized + Send,
    TMethod: for<'a>Fn(&'a mut TActor, &'a TInput) ->TFuture
{
    fn new(method: TMethod) -> Self {
        ActorMethodContainer {
            method: Arc::new(Mutex::new(method)),
            _actor: std::marker::PhantomData::<TActor>::default(),
            _input: std::marker::PhantomData::<TInput>::default(),
            _output: std::marker::PhantomData::<TOutput>::default(),
            //_lifetime: std::marker::PhantomData,            
            //_future: std::marker::PhantomData::<TFuture>::default(),
        }
    }
}

impl<TActor, TInput, TMethod, TOutput, TFuture> ActorMethod for ActorMethodContainer<TActor, TInput, TMethod, TOutput, TFuture>  
where 
    TActor: Actor + Unpin + 'static, 
    TInput: for<'a> Deserialize<'a> + Send + 'static, 
    TOutput: Serialize + 'static,
    TFuture: Future<Output = Result<TOutput, ActorError>> + Sized + Send + 'static,
    TMethod: for<'a>Fn(&'a mut TActor, &'a TInput) -> TFuture + 'static
{

    fn build(&self, actor: Arc<Mutex<Box<dyn Actor>>>, data: Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>> {
        //let m = self.method.as_ref();
        //m.
        let fm = DecoratedActorMethod {
            input: None,
            method_future: None,
            method: self.method.clone(),
            actor: actor,
            serialized_input: Box::new(data),
            _phantom: std::marker::PhantomData::<TActor>::default()
        };
        
        Box::pin(fm)
    }
}


pub struct DecoratedActorMethod<'b, TActor, TInput, TMethod, TOutput, TFuture> 
where 
    TActor: Actor + Unpin + 'b, 
    TInput: for<'a> Deserialize<'a> + 'b, 
    TOutput: Serialize + 'b,
    TFuture: Future<Output = Result<TOutput, ActorError>> + Sized,
    TMethod: for<'a>Fn(&'a mut TActor, &'a TInput) -> TFuture
{
    input: Option<Pin<Box<TInput>>>,
    method_future: Option<BoxFuture<'b, Result<TOutput, ActorError>>>,
    method: Arc<Mutex<TMethod>>,
    actor: Arc<Mutex<Box<dyn Actor>>>, //Arc<Mutex<TActor>>,
    serialized_input: Box<Vec<u8>>,
    _phantom: std::marker::PhantomData<TActor>
}

impl<TActor, TInput, TMethod, TOutput, TFuture> Future for DecoratedActorMethod<'static, TActor, TInput, TMethod, TOutput, TFuture> 
where 
    TActor: Actor + Unpin, 
    TInput: for<'a> Deserialize<'a>, 
    TOutput: Serialize,
    TFuture: Future<Output = Result<TOutput, ActorError>> + Sized + Send + 'static,
    TMethod: for<'a>Fn(&'a mut TActor, &'a TInput) -> TFuture
{
    type Output = Result<Vec<u8>, ActorError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut this = self.get_mut();
        
        if let None = this.input {
            let args = serde_json::from_slice::<TInput>(&this.serialized_input);
            if args.is_err() {
                log::error!("Failed to deserialize actor method arguments - {:?}", args.err());
                return Poll::Ready(Err(ActorError::SerializationError()));
            }

            this.input = Some(Box::pin(args.unwrap()));
        }
        
        if let None = this.method_future {
            //let method_ref = this.method; //.as_ref();
            let input_ref = this.input.as_ref().unwrap().as_ref().get_ref();
            let m = this.method.lock().unwrap();
            let mut actor = this.actor.lock().unwrap();
            let a2 = actor.as_mut();
            
            let well_known_actor = unsafe { &mut *(a2 as *mut dyn Actor as *mut TActor) };
            
            let fut = m(well_known_actor, input_ref);            
            this.method_future = Some(Box::pin(fut));
        }

        match this.method_future.as_mut().unwrap().poll_unpin(cx) {
            Poll::Ready(result) => match result {
                Ok(r) => {
                    let serialized = serde_json::to_vec(&r).unwrap();
                    Poll::Ready(Ok(serialized))
                },
                Err(e) => Poll::Ready(Err(e))
            },
            Poll::Pending => Poll::Pending,
        }
    }
}