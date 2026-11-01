//! Session manager: the lifecycle of every session (created, starting, running,
//! waiting on user, idle, disconnected, ended). Routes user input and permission
//! decisions to adapters, and raw backend output to the raw log.
