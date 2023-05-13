use std::{sync::Arc, sync::Mutex, error::Error, pin::Pin, task::Poll};
use futures::{Future, FutureExt};
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


pub type ActorMethod = dyn Fn(Arc<Mutex<Box<dyn Actor>>>, Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>>;

//for<'a, 'b> Fn<(&'a mut (dyn Actor + 'a), &'b Vec<u8>)>


// pub fn decorate_actor_method<'b, TActor, TInput, TMethod, TOutput, TFuture>(method: TMethod) -> Box<ActorMethod>
//     where 
//         TActor: Actor + Send + Sync, 
//         TInput: for<'a> Deserialize<'a> + Send + Sync, 
//         TOutput: Serialize + Send + Sync,
//         TFuture: Future<Output = Result<TOutput, ActorError>> + Sized + Send + Sync,
//         TMethod: Fn(&mut TActor, TInput) -> TFuture + Sync + 'static
// {       
//     let m2 = &method;
//     let f =  move |actor: Pin<Box<dyn Actor>>, data: Vec<u8>| -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>> {
//         log::debug!("Invoking actor method with data: {:?}", data);        
//         let args = serde_json::from_slice::<TInput>(&data);
//         if args.is_err() {
//             log::error!("Failed to deserialize actor method arguments - {:?}", args.err());
//             return async { Err(ActorError::SerializationError()) }.boxed();
//         }

//         let zz = async move { 

//             let a2 = unsafe { actor.as_mut().get_unchecked_mut() };


//             let well_known_actor = unsafe { &mut *(a2 as *mut dyn Actor as *mut TActor) };

//             match m2(well_known_actor, args.unwrap()).await {
//                 Ok(r) => {
//                     let serialized = serde_json::to_vec(&r).unwrap();
//                     Ok(serialized)
//                 },
//                 Err(e) => Err(e)
//             }
//         };

//         zz.boxed()
//     };

//     Box::new(f)
// }

pub struct DecoratedActorMethod<TActor, TInput, TMethod, TOutput, TFuture> 
where 
    TActor: Actor + Unpin, 
    TInput: for<'a> Deserialize<'a>, 
    TOutput: Serialize,
    TFuture: Future<Output = Result<TOutput, ActorError>> + Sized,
    TMethod: Fn(&mut TActor, &TInput) -> TFuture
{
    input: Option<Pin<Box<TInput>>>,
    method_future: Option<Pin<Box<TFuture>>>,
    method: Arc<Mutex<TMethod>>,
    actor: Arc<Mutex<Box<dyn Actor>>>, //Arc<Mutex<TActor>>,
    serialized_input: Box<Vec<u8>>,
    _phantom: std::marker::PhantomData<TActor>
}


// impl<'b, TActor, TInput, TMethod, TOutput, TFuture> DecoratedActorMethod<'b, TActor, TInput, TMethod, TOutput, TFuture> 
// where 
//     TActor: Actor + 'b,
//     TInput: for<'a> Deserialize<'a> + 'b, 
//     TOutput: Serialize + 'b,
//     TFuture: Future<Output = Result<TOutput, ActorError>> + Unpin + 'b,
//     TMethod: Fn(&mut TActor, &TInput) -> TFuture + 'b
// {
//     pub fn factory(method: TMethod) -> &'b dyn Fn(&mut dyn Actor, &Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>> {
        
//         let f = move |actor: &'b mut dyn Actor, data: &'b Vec<u8>| {
            
//             let well_known_actor = unsafe { &mut *(actor as *mut dyn Actor as *mut TActor) };
            
            
//             let fm = DecoratedActorMethod {
//                 input: None,
//                 method_future: None,
//                 method: Box::new(&method),
//                 actor: Box::new(well_known_actor),
//                 serialized_input: Box::new(data),
//             };            
//             //let fm3 = unsafe { *(&fm as *const dyn Future<Output = Result<Vec<u8>, ActorError>>) };
            
//             let b1 = Box::pin(fm);
//             let c = b1 as Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>>;

//             //let fm2: &(dyn Future<Output = Result<Vec<u8>, ActorError>>) = &fm as &dyn Future<Output = Result<Vec<u8>, ActorError>>;
            
//             //Box::pin(fm2)
//             c
//         };

//         let cap: &'b dyn Fn(&mut dyn Actor, &Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ActorError>>>> = &f;
//         cap
//     }
// }

impl<TActor, TInput, TMethod, TOutput, TFuture> Future for DecoratedActorMethod<TActor, TInput, TMethod, TOutput, TFuture> 
where 
    TActor: Actor + Unpin, 
    TInput: for<'a> Deserialize<'a>, 
    TOutput: Serialize,
    TFuture: Future<Output = Result<TOutput, ActorError>> + Sized,
    TMethod: Fn(&mut TActor, &TInput) -> TFuture
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